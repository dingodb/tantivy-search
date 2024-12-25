use std::sync::Arc;

use rayon::range;
use roaring::{RoaringBitmap, RoaringTreemap};
use tantivy::query::{Query, QueryParser, QueryParserError, RegexQuery, TermQuery, TermSetQuery};
use tantivy::schema::{Field, FieldType, IndexRecordOption, TextFieldIndexing};
use tantivy::tokenizer::{BoxTokenStream, TextAnalyzer};
use tantivy::{schema::Schema, Searcher};
use tantivy::{TantivyError, Term};

use crate::common::constants::LOG_CALLBACK;
use crate::logger::logger_bridge::TantivySearchLogger;
use crate::search::collector::row_id_bitmap_collector::RowIdRoaringCollector;
use crate::search::collector::top_docs_with_bitmap_collector::TopDocsWithFilter;
use crate::search::collector::top_docs_with_treemap_collector::TopDocsWithFilter64;
use crate::search::collector::unlimited_docs_with_treemap_collector::UnlimitedDocsWithFilter64;
use crate::search::utils::convert_utils::ConvertUtils;
use crate::INFO;
use crate::{common::errors::IndexSearcherError, ffi::RowIdWithScore, ERROR};

pub trait QueryStrategy<T> {
    fn execute(&self, searcher: &Searcher) -> Result<T, IndexSearcherError>;
}

/// Execute query for a group of terms.
///
/// Params:
/// - `column_name`: Execute query in which column.
/// - `terms`: A group of terms.
///
pub struct TermSetQueryStrategy<'a> {
    pub column_name: &'a str,
    pub terms: &'a Vec<String>,
}

impl<'a> QueryStrategy<Arc<RoaringBitmap>> for TermSetQueryStrategy<'a> {
    fn execute(&self, searcher: &Searcher) -> Result<Arc<RoaringBitmap>, IndexSearcherError> {
        let schema: Schema = searcher.index().schema();

        let col_field: Field = schema.get_field(self.column_name).map_err(|e| {
            let error: IndexSearcherError = IndexSearcherError::TantivyError(e);
            ERROR!(function:"TermSetQueryStrategy", "{}", error);
            error
        })?;

        let field_type: &FieldType = schema.get_field_entry(col_field).field_type();
        if !field_type.is_indexed() {
            let error_msg: String = format!("column field:{} not indexed.", self.column_name);
            ERROR!(function:"TermSetQueryStrategy", "{}", error_msg);
            return Err(IndexSearcherError::InternalError(error_msg));
        }

        let mut terms: Vec<Term> = Vec::new();

        if let FieldType::Str(ref str_options) = field_type {
            let indexing_options: &TextFieldIndexing =
                str_options.get_indexing_options().ok_or_else(|| {
                    let error_msg: String = format!(
                        "column field:{} not indexed, but this error msg shouldn't display",
                        self.column_name
                    );
                    ERROR!(function:"TermSetQueryStrategy", "{}", error_msg);
                    IndexSearcherError::InternalError(error_msg)
                })?;
            let mut text_analyzer: TextAnalyzer = searcher
                .index()
                .tokenizers()
                .get(indexing_options.tokenizer())
                .unwrap();

            for term in self.terms {
                let mut token_stream: BoxTokenStream<'_> = text_analyzer.token_stream(term);
                token_stream.process(&mut |token| {
                    terms.push(Term::from_field_text(col_field, &token.text));
                });
            }
        } else {
            // Not Expected.
            for term in self.terms {
                terms.push(Term::from_field_text(col_field, &term));
            }
        }

        let ter_set_query: TermSetQuery = TermSetQuery::new(terms);
        let row_id_collector: RowIdRoaringCollector =
            RowIdRoaringCollector::with_field("row_id".to_string());

        searcher
            .search(&ter_set_query, &row_id_collector)
            .map_err(|e| {
                ERROR!(function:"SingleTermQueryStrategy", "{}", e);
                IndexSearcherError::TantivyError(e)
            })
    }
}

/// Execute query for one term.
///
/// Params:
/// - `column_name`: Execute query in which column.
/// - `term`: Term need to be queried.
///
pub struct SingleTermQueryStrategy<'a> {
    pub column_name: &'a str,
    pub term: &'a str,
}

impl<'a> QueryStrategy<Arc<RoaringBitmap>> for SingleTermQueryStrategy<'a> {
    fn execute(&self, searcher: &Searcher) -> Result<Arc<RoaringBitmap>, IndexSearcherError> {
        let schema: Schema = searcher.index().schema();

        let col_field: Field = schema.get_field(self.column_name).map_err(|e| {
            let error: IndexSearcherError = IndexSearcherError::TantivyError(e);
            ERROR!(function:"SingleTermQueryStrategy", "{}", error);
            error
        })?;

        let field_type: &FieldType = schema.get_field_entry(col_field).field_type();
        if !field_type.is_indexed() {
            let error_msg: String = format!("column field:{} not indexed.", self.column_name);
            ERROR!(function:"SingleTermQueryStrategy", "{}", error_msg);
            return Err(IndexSearcherError::InternalError(error_msg));
        }

        if let FieldType::Str(ref str_options) = field_type {
            let indexing_options: &TextFieldIndexing =
                str_options.get_indexing_options().ok_or_else(|| {
                    let error_msg: String = format!(
                        "column field:{} not indexed, but this error msg shouldn't display",
                        self.column_name
                    );
                    ERROR!(function:"SingleTermQueryStrategy", "{}", error_msg);
                    IndexSearcherError::InternalError(error_msg)
                })?;
            let mut terms: Vec<Term> = Vec::new();
            let mut text_analyzer: TextAnalyzer = searcher
                .index()
                .tokenizers()
                .get(indexing_options.tokenizer())
                .unwrap();
            let mut token_stream: BoxTokenStream<'_> = text_analyzer.token_stream(self.term);
            token_stream.process(&mut |token| {
                let term: Term = Term::from_field_text(col_field, &token.text);
                terms.push(term);
            });

            let ter_set_query: TermSetQuery = TermSetQuery::new(terms);
            let row_id_collector: RowIdRoaringCollector =
                RowIdRoaringCollector::with_field("row_id".to_string());

            searcher
                .search(&ter_set_query, &row_id_collector)
                .map_err(|e| {
                    ERROR!(function:"SingleTermQueryStrategy", "{}", e);
                    IndexSearcherError::TantivyError(e)
                })
        } else {
            // Not Expected.
            let term: Term = Term::from_field_text(col_field, self.term);
            let term_query: TermQuery = TermQuery::new(term, IndexRecordOption::WithFreqs);
            let row_id_collector: RowIdRoaringCollector =
                RowIdRoaringCollector::with_field("row_id".to_string());
            println!("for not str");
            searcher
                .search(&term_query, &row_id_collector)
                .map_err(|e| {
                    ERROR!(function:"SingleTermQueryStrategy", "{}", e);
                    IndexSearcherError::TantivyError(e)
                })
        }
    }
}

/// Execute regex query for a given pattern.
///
/// Params:
/// - `column_name`: Execute query in which column.
/// - `pattern`: Regex query will execute with given pattern str.
///
pub struct RegexQueryStrategy<'a> {
    pub column_name: &'a str,
    pub pattern: &'a str,
}

impl<'a> QueryStrategy<Arc<RoaringBitmap>> for RegexQueryStrategy<'a> {
    fn execute(&self, searcher: &Searcher) -> Result<Arc<RoaringBitmap>, IndexSearcherError> {
        let schema: Schema = searcher.index().schema();

        let col_field: Field = schema.get_field(self.column_name).map_err(|e| {
            let error: IndexSearcherError = IndexSearcherError::TantivyError(e);
            ERROR!(function:"RegexQueryStrategy", "{}", error);
            error
        })?;

        let row_id_collector: RowIdRoaringCollector =
            RowIdRoaringCollector::with_field("row_id".to_string());
        let regex_query: RegexQuery = RegexQuery::from_pattern(&ConvertUtils::like_to_regex(self.pattern), col_field).map_err(|e|{
            ERROR!(function:"RegexQueryStrategy", "Error when parse regex query:{}. {}", ConvertUtils::like_to_regex(self.pattern), e);
            IndexSearcherError::TantivyError(e)
        })?;

        searcher.search(&regex_query, &row_id_collector).map_err(|e|{
            ERROR!(function:"RegexQueryStrategy", "Error when execute regex query:{}. {}", ConvertUtils::like_to_regex(self.pattern), e);
            IndexSearcherError::TantivyError(e)
        })
    }
}

/// Execute query for a sentence, without natural language search.
/// This sentence can be written by natural language, or just simple terms.
/// It will convert to terms query when execute.
///
/// Params:
/// - `column_name`: Execute query in which column.
/// - `sentence`: Sentence need to query.
///
pub struct SentenceQueryStrategy<'a> {
    pub column_name: &'a str,
    pub sentence: &'a str,
}

impl<'a> QueryStrategy<Arc<RoaringBitmap>> for SentenceQueryStrategy<'a> {
    fn execute(&self, searcher: &Searcher) -> Result<Arc<RoaringBitmap>, IndexSearcherError> {
        let schema: Schema = searcher.index().schema();

        let col_field: Field = schema.get_field(self.column_name).map_err(|e| {
            let error: IndexSearcherError = IndexSearcherError::TantivyError(e);
            ERROR!(function:"SentenceQueryStrategy", "{}", error);
            error
        })?;

        let field_type: &FieldType = schema.get_field_entry(col_field).field_type();
        if !field_type.is_indexed() {
            let error_msg: String = format!("column field:{} not indexed.", self.column_name);
            ERROR!(function:"SentenceQueryStrategy", "{}", error_msg);
            return Err(IndexSearcherError::InternalError(error_msg));
        }

        let mut terms: Vec<Term> = Vec::new();

        if let FieldType::Str(ref str_options) = field_type {
            let indexing_options: &TextFieldIndexing =
                str_options.get_indexing_options().ok_or_else(|| {
                    let error_msg: String = format!(
                        "column field:{} not indexed, but this error msg shouldn't display",
                        self.column_name
                    );
                    ERROR!(function:"SentenceQueryStrategy", "{}", error_msg);
                    IndexSearcherError::InternalError(error_msg)
                })?;

            let mut text_analyzer: TextAnalyzer = searcher
                .index()
                .tokenizers()
                .get(indexing_options.tokenizer())
                .unwrap();

            let mut token_stream: BoxTokenStream<'_> = text_analyzer.token_stream(self.sentence);
            token_stream.process(&mut |token| {
                terms.push(Term::from_field_text(col_field, &token.text));
            });
        } else {
            let error_msg = "Not expected, column field type must be str type.";
            ERROR!(function:"SentenceQueryStrategy", "{}", error_msg);
            return Err(IndexSearcherError::InternalError(error_msg.to_string()));
        }

        let ter_set_query: TermSetQuery = TermSetQuery::new(terms);
        let row_id_collector: RowIdRoaringCollector =
            RowIdRoaringCollector::with_field("row_id".to_string());

        searcher
            .search(&ter_set_query, &row_id_collector)
            .map_err(|e| {
                ERROR!(function:"SentenceQueryStrategy", "{}", e);
                IndexSearcherError::TantivyError(e)
            })
    }
}

/// Execute query for a sentence.
/// This sentence may be written by natural language, or just simple terms.
///
/// Params:
/// - `column_name`: Execute query in which column.
/// - `sentence`: Sentence need to be parsed and query.
///
pub struct ParserQueryStrategy<'a> {
    pub column_name: &'a str,
    pub sentence: &'a str,
}

impl<'a> QueryStrategy<Arc<RoaringBitmap>> for ParserQueryStrategy<'a> {
    fn execute(&self, searcher: &Searcher) -> Result<Arc<RoaringBitmap>, IndexSearcherError> {
        let schema: Schema = searcher.index().schema();

        let col_field: Field = schema.get_field(self.column_name).map_err(|e| {
            let error: IndexSearcherError = IndexSearcherError::TantivyError(e);
            ERROR!(function:"SingleTermQueryStrategy", "{}", error);
            error
        })?;

        let row_id_collector: RowIdRoaringCollector =
            RowIdRoaringCollector::with_field("row_id".to_string());
        let query_parser: QueryParser =
            QueryParser::for_index(searcher.index(), [col_field].to_vec());

        let text_query = query_parser.parse_query(self.sentence).map_err(|e| {
            ERROR!(function:"ParserQueryStrategy", "Error when parse: {}. {}", self.sentence, e);
            IndexSearcherError::QueryParserError(e.to_string())
        })?;

        searcher.search(&text_query, &row_id_collector).map_err(|e|{
            ERROR!(function:"ParserQueryStrategy", "Error when execute: {}. {}", self.sentence, e);
            IndexSearcherError::TantivyError(e)
        })
    }
}

/// Execute query for a sentence and get bm25 score.
/// Query will be run in all schema fields but `row_id`.
/// This sentence may be written by natural language, or just simple terms.
/// If `query_with_filter` is true, when calculating bm25 score, only in `alive_row_ids` will be recorded.
///
/// Params:
/// - `sentence`: Sentence need to be parsed and query.
/// - `topk`: max-heap build with topK
/// - `u8_aived_bitmap`: Represent row_ids who are alived.
/// - `query_with_filter`: Whether collect row_ids with `u8_alived_bitmap`
///
pub struct BM25QueryStrategy<'a> {
    pub sentence: &'a str,
    pub topk: &'a u32,
    pub alived_ids: &'a Vec<u32>,
    pub query_with_filter: &'a bool,
    pub need_doc: &'a bool,
    pub column_names: &'a Vec<String>,
}

impl<'a> QueryStrategy<Vec<RowIdWithScore>> for BM25QueryStrategy<'a> {
    fn execute(&self, searcher: &Searcher) -> Result<Vec<RowIdWithScore>, IndexSearcherError> {
        let schema: Schema = searcher.index().schema();
        let filed_num = schema.num_fields();

        let fields: Vec<Field> = match self.column_names.is_empty() {
            true => schema
                .fields()
                .filter(|(field, _)| {
                    schema.get_field_name(*field) != "row_id" && {
                        match schema.get_field_entry(*field).field_type() {
                            tantivy::schema::FieldType::Str(_) => true,
                            _ => false,
                        }
                    }
                })
                .map(|(field, _)| field)
                .collect(),
            false => self
                .column_names
                .iter()
                .filter(|col_name| match schema.get_field(*col_name) {
                    Ok(field) => match schema.get_field_entry(field).field_type() {
                        tantivy::schema::FieldType::Str(_) => true,
                        _ => false,
                    },
                    Err(e) => {
                        let error: IndexSearcherError = IndexSearcherError::TantivyError(e);
                        ERROR!(function:"BM25QueryStrategy", "{}", error);
                        return false;
                    }
                })
                .map(|col_name| {
                    schema.get_field(col_name).map_err(|e| {
                        let error: IndexSearcherError = IndexSearcherError::TantivyError(e);
                        ERROR!(function:"BM25QueryStrategy", "{}", error);
                        error
                    })
                })
                .collect::<Result<Vec<Field>, IndexSearcherError>>()?,
        };

        INFO!(function:"BM25QueryStrategy", "Fields: {:?}", fields);

        let mut top_docs_collector: TopDocsWithFilter =
            TopDocsWithFilter::with_limit(*self.topk as usize)
                .with_searcher(searcher.clone())
                .with_text_fields(fields.clone())
                .with_stored_text(*self.need_doc);

        // If query_with_filter is false, we regards that don't use alive_bitmap.
        // if *self.query_with_filter {
        //     let mut alive_bitmap: RoaringBitmap = RoaringBitmap::new();
        //     alive_bitmap.extend(ConvertUtils::u8_bitmap_to_row_ids(self.u8_aived_bitmap));
        //     top_docs_collector = top_docs_collector.with_alive(Arc::new(alive_bitmap));
        // }
        if *self.query_with_filter {
            let mut alive_bitmap: RoaringBitmap = RoaringBitmap::new();
            alive_bitmap.extend(self.alived_ids);
            top_docs_collector = top_docs_collector.with_alive(Arc::new(alive_bitmap));
        }

        let query_parser: QueryParser = QueryParser::for_index(searcher.index(), fields);
        let text_query: Box<dyn Query> = query_parser.parse_query(self.sentence).map_err(
            |e: QueryParserError| {
                ERROR!(function:"BM25QueryStrategy", "Error when parse: {}. {}", self.sentence, e);
                IndexSearcherError::QueryParserError(e.to_string())
            },
        )?;

        searcher.search(&text_query, &top_docs_collector).map_err(|e: TantivyError|{
            ERROR!(function:"BM25QueryStrategy", "Error when execute: {}. {}", self.sentence, e);
            IndexSearcherError::TantivyError(e)
        })
    }
}

/// Execute query for a sentence and get bm25 score.
/// Query will be run in all schema fields but `row_id`.
/// This sentence may be written by natural language, or just simple terms.
/// If `query_with_filter` is true, when calculating bm25 score, only in `alive_row_ids` will be recorded.
///
/// Params:
/// - `sentence`: Sentence need to be parsed and query.
/// - `topk`: max-heap build with topK
/// - `u8_aived_bitmap`: Represent row_ids who are alived.
/// - `query_with_filter`: Whether collect row_ids with `u8_alived_bitmap`
/// - `query_with_id_range`: Whether collect row_ids with `[start_id, end_id)`
/// - `start_id`: The start of row_ids range
/// - `end_id`: The end of row_ids range
///
pub struct BM25QueryStrategy64<'a> {
    pub sentence: &'a str,
    pub topk: &'a u32,
    pub alived_ids: &'a Vec<u64>,
    pub query_with_filter: &'a bool,
    pub query_with_id_range: &'a bool,
    pub start_id: &'a u64,
    pub end_id: &'a u64,
    pub need_doc: &'a bool,
    pub column_names: &'a Vec<String>,
    pub query_unlimited: &'a bool,
}

impl<'a> QueryStrategy<Vec<RowIdWithScore>> for BM25QueryStrategy64<'a> {
    fn execute(&self, searcher: &Searcher) -> Result<Vec<RowIdWithScore>, IndexSearcherError> {
        let schema: Schema = searcher.index().schema();
        let fields: Vec<Field> = match self.column_names.is_empty() {
            true => schema
                .fields()
                .filter(|(field, _)| {
                    schema.get_field_name(*field) != "row_id" && {
                        match schema.get_field_entry(*field).field_type() {
                            tantivy::schema::FieldType::Str(_) => true,
                            _ => false,
                        }
                    }
                })
                .map(|(field, _)| field)
                .collect(),
            false => self
                .column_names
                .iter()
                .filter(|col_name| match schema.get_field(*col_name) {
                    Ok(field) => match schema.get_field_entry(field).field_type() {
                        tantivy::schema::FieldType::Str(_) => true,
                        _ => false,
                    },
                    Err(e) => {
                        let error: IndexSearcherError = IndexSearcherError::TantivyError(e);
                        ERROR!(function:"BM25QueryStrategy64", "{}", error);
                        return false;
                    }
                })
                .map(|col_name| {
                    schema.get_field(col_name).map_err(|e| {
                        let error: IndexSearcherError = IndexSearcherError::TantivyError(e);
                        ERROR!(function:"BM25QueryStrategy64", "{}", error);
                        error
                    })
                })
                .collect::<Result<Vec<Field>, IndexSearcherError>>()?,
        };

        INFO!(function:"BM25QueryStrategy64", "Fields: {:?}", fields);

        if *self.query_unlimited {
            let mut top_docs_collector: UnlimitedDocsWithFilter64 =
                UnlimitedDocsWithFilter64::with_default()
                    .with_searcher(searcher.clone())
                    .with_text_fields(fields.clone())
                    .with_stored_text(*self.need_doc);
            if *self.query_with_filter {
                let mut alive_bitmap: RoaringTreemap = RoaringTreemap::new();
                alive_bitmap.extend(self.alived_ids);
                top_docs_collector = top_docs_collector.with_alive(Arc::new(alive_bitmap));
            }

            if *self.query_with_id_range {
                top_docs_collector = top_docs_collector.with_range((*self.start_id, *self.end_id));
            }

            let query_parser: QueryParser = QueryParser::for_index(searcher.index(), fields);
            let text_query: Box<dyn Query> = query_parser.parse_query(self.sentence).map_err(
                    |e: QueryParserError| {
                        ERROR!(function:"BM25QueryStrategy64", "Error when parse: {}. {}", self.sentence, e);
                        IndexSearcherError::QueryParserError(e.to_string())
                    },
                )?;

            return searcher.search(&text_query, &top_docs_collector).map_err(|e: TantivyError|{
                    ERROR!(function:"BM25QueryStrategy64", "Error when execute: {}. {}", self.sentence, e);
                    IndexSearcherError::TantivyError(e)
                });
        }
        let mut top_docs_collector: TopDocsWithFilter64 =
            TopDocsWithFilter64::with_limit(*self.topk as usize)
                .with_searcher(searcher.clone())
                .with_text_fields(fields.clone())
                .with_stored_text(*self.need_doc);
        if *self.query_with_filter {
            let mut alive_bitmap: RoaringTreemap = RoaringTreemap::new();
            alive_bitmap.extend(self.alived_ids);
            top_docs_collector = top_docs_collector.with_alive(Arc::new(alive_bitmap));
        }

        if *self.query_with_id_range {
            top_docs_collector = top_docs_collector.with_range((*self.start_id, *self.end_id));
        }

        let query_parser: QueryParser = QueryParser::for_index(searcher.index(), fields);
        let text_query: Box<dyn Query> = query_parser.parse_query(self.sentence).map_err(
            |e: QueryParserError| {
                ERROR!(function:"BM25QueryStrategy64", "Error when parse: {}. {}", self.sentence, e);
                IndexSearcherError::QueryParserError(e.to_string())
            },
        )?;

        searcher.search(&text_query, &top_docs_collector).map_err(|e: TantivyError|{
            ERROR!(function:"BM25QueryStrategy64", "Error when execute: {}. {}", self.sentence, e);
            IndexSearcherError::TantivyError(e)
        })
    }
}

pub struct QueryExecutor<'a, T> {
    strategy: &'a dyn QueryStrategy<T>,
}

impl<'a, T> QueryExecutor<'a, T> {
    pub fn new(strategy: &'a dyn QueryStrategy<T>) -> Self {
        QueryExecutor { strategy }
    }
    pub fn execute(&self, searcher: &Searcher) -> Result<T, IndexSearcherError> {
        self.strategy.execute(searcher)
    }
}
