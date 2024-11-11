use std::collections::BinaryHeap;
use std::sync::Arc;
use std::{cmp, fmt};

use roaring::RoaringTreemap;
use tantivy::collector::{Collector, SegmentCollector};
use tantivy::query::Weight;
use tantivy::schema::{Field, Value};
use tantivy::{DocAddress, DocId, Score, Searcher, SegmentOrdinal, SegmentReader, TantivyDocument};

use crate::RowIdWithScore;

// Class Inheritance Diagram:
//
//   +---------------------+            +--------------------------+
//   | UnlimitedDocsWithFilter64   |<-----------| TopScoreSegmentCollector |
//   +---------------------+            +--------------------------+
//
// Variables in TopDocWithFilter:
// @`searcher` is an Option type used to read the original text stored in the index.
// @`text_fields` is an Option type from which the `searcher` reads the original text stored in the index.
// @`need_text` indicates whether the original text needs to be read from the index. If this is true, but either `searcher` or `text_fields` is None, the original text will not be retrieved.
#[derive(Default)]
pub struct UnlimitedDocsWithFilter64 {
    pub row_id_treemap: Option<Arc<RoaringTreemap>>,
    pub row_id_range: Option<(u64, u64)>,
    pub searcher: Option<Searcher>,
    pub text_fields: Option<Vec<Field>>,
    pub need_text: bool,
}

impl UnlimitedDocsWithFilter64 {

    pub fn with_default() -> UnlimitedDocsWithFilter64 {
        Self {
            row_id_treemap: None,
            row_id_range: None,
            searcher: None,
            text_fields: None,
            need_text: false,
        }
    }

    // `row_id_bitmap` is used to mark aive row_ids.
    pub fn with_alive(mut self, row_id_bitmap: Arc<RoaringTreemap>) -> UnlimitedDocsWithFilter64 {
        self.row_id_treemap = Some(Arc::clone(&row_id_bitmap));
        self
    }

    // `row_id_range` is used to mark alive row_id range, the range is [start, end)
    pub fn with_range(mut self, row_id_range: (u64, u64)) -> UnlimitedDocsWithFilter64 {
        self.row_id_range = Some(row_id_range);
        self
    }

    // `searcher` is used to search origin text content.
    pub fn with_searcher(mut self, searcher: Searcher) -> UnlimitedDocsWithFilter64 {
        self.searcher = Some(searcher.clone());
        self
    }

    // field which store origin text content.
    pub fn with_text_fields(mut self, fields: Vec<Field>) -> UnlimitedDocsWithFilter64 {
        self.text_fields = Some(fields.clone());
        self
    }

    // whether need return origin text content.
    pub fn with_stored_text(mut self, need_text: bool) -> UnlimitedDocsWithFilter64 {
        self.need_text = need_text;
        self
    }

    pub fn merge_fruits(
        &self,
        children: Vec<Vec<RowIdWithScore>>,
    ) -> tantivy::Result<Vec<RowIdWithScore>> {
        let mut total_collector = BinaryHeap::new();
        for child_fruit in children {
            for child in child_fruit {
                total_collector.push(child);
            }
        }
        Ok(total_collector.into_sorted_vec())
    }

    #[inline]
    fn extract_doc_text(&self, doc: DocId, segment_ord: SegmentOrdinal) -> Vec<String> {
        let mut doc_texts: Vec<String> = vec![];
        if self.need_text {
            if let Some(searcher) = &self.searcher {
                if let Ok(document) = searcher.doc::<TantivyDocument>(DocAddress {
                    segment_ord,
                    doc_id: doc,
                }) {
                    if let Some(text_fields) = &self.text_fields {
                        for text_field in text_fields {
                            if let Some(field_value) = document.get_first(*text_field) {
                                if let Some(text_value) = field_value.as_str() {
                                    doc_texts.push(text_value.to_string());
                                } else {
                                    doc_texts.push("".to_string())
                                }
                            }
                        }
                    }
                }
            }
        }
        doc_texts
    }
}

impl fmt::Debug for UnlimitedDocsWithFilter64 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "UnlimitedDocsWithFilter64(row_ids_size:{}, row_id_range_start:{} row_id_range_end:{} text_fields_is_some:{}, searcher_is_some:{}, need_text:{})",
            if self.row_id_treemap.is_some() {self.row_id_treemap.clone().unwrap().len()} else {0},
            if self.row_id_range.is_some() {self.row_id_range.clone().unwrap().0} else {0},
            if self.row_id_range.is_some() {self.row_id_range.clone().unwrap().1} else {0},
            self.text_fields.is_some(),
            self.searcher.is_some(),
            self.need_text,
        )
    }
}

impl Collector for UnlimitedDocsWithFilter64 {
    type Fruit = Vec<RowIdWithScore>;

    type Child = UnlimitedDocsWithFilter64SegmentCollector; // won't use for current design.

    // won't use for current design.
    fn for_segment(
        &self,
        _segment_local_id: SegmentOrdinal,
        _reader: &SegmentReader,
    ) -> tantivy::Result<Self::Child> {
        Ok(UnlimitedDocsWithFilter64SegmentCollector())
    }

    // won't use for current design.
    fn requires_scoring(&self) -> bool {
        true
    }

    fn merge_fruits(&self, child_fruits: Vec<Vec<RowIdWithScore>>) -> tantivy::Result<Self::Fruit> {
        self.merge_fruits(child_fruits)
    }

    // collector for each segment.
    fn collect_segment(
        &self,
        weight: &dyn Weight,
        segment_ord: SegmentOrdinal,
        reader: &SegmentReader,
    ) -> tantivy::Result<<Self::Child as SegmentCollector>::Fruit> {
        let mut vec_row_ids_with_scores: Vec<RowIdWithScore> = vec![];
        let row_id_field_reader = reader
            .fast_fields()
            .u64("row_id")
            .unwrap()
            .first_or_default_col(0);

        if let Some(alive_bitset) = reader.alive_bitset() {
            weight.for_each(reader, &mut |doc, score| {
                let row_id = row_id_field_reader.get_val(doc);
                if self.row_id_treemap.is_some()
                    && !self.row_id_treemap.clone().unwrap().contains(row_id)
                {
                    return;
                }
                if self.row_id_range.is_some()
                    && !(self.row_id_range.clone().unwrap().0 <= row_id
                        && row_id < self.row_id_range.clone().unwrap().1)
                {
                    return;
                }
                if alive_bitset.is_deleted(doc) {
                    return;
                }
                let vec_item = RowIdWithScore {
                    row_id,
                    score,
                    seg_id: segment_ord,
                    doc_id: doc,
                    docs: self.extract_doc_text(doc, segment_ord),
                };
                vec_row_ids_with_scores.push(vec_item);
            })?;
        } else {
            weight.for_each(reader, &mut |doc, score| {
                let row_id = row_id_field_reader.get_val(doc);
                if self.row_id_treemap.is_some()
                    && !self.row_id_treemap.clone().unwrap().contains(row_id)
                {
                    return;
                }
                if self.row_id_range.is_some()
                    && !(self.row_id_range.clone().unwrap().0 <= row_id
                        && row_id < self.row_id_range.clone().unwrap().1)
                {
                    return;
                }
                let vec_item = RowIdWithScore {
                    row_id,
                    score,
                    seg_id: segment_ord,
                    doc_id: doc,
                    docs: self.extract_doc_text(doc, segment_ord),
                };
                vec_row_ids_with_scores.push(vec_item);
            })?;
        }
        Ok(vec_row_ids_with_scores)
    }
}

pub struct UnlimitedDocsWithFilter64SegmentCollector();

impl SegmentCollector for UnlimitedDocsWithFilter64SegmentCollector {
    type Fruit = Vec<RowIdWithScore>;

    fn collect(&mut self, _doc: DocId, _score: Score) {
        println!("Not implement");
    }

    fn harvest(self) -> Vec<RowIdWithScore> {
        println!("Not implement");
        vec![]
    }
}
