use std::str::Utf8Error;

use cxx::{vector::VectorElement, CxxString, CxxVector};

use super::errors::CxxConvertError;

use tantivy::DateTime;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub trait ConvertStrategy<T, U> {
    fn convert(&self, item: &T) -> Result<U, CxxConvertError>;
}

// u8->u8, u32->u32
pub struct CxxElementStrategy;

impl<T> ConvertStrategy<T, T> for CxxElementStrategy
where
    T: Clone,
{
    fn convert(&self, item: &T) -> Result<T, CxxConvertError> {
        Ok(item.clone())
    }
}

impl ConvertStrategy<CxxString, String> for CxxElementStrategy {
    fn convert(&self, item: &CxxString) -> Result<String, CxxConvertError> {
        let result: Result<String, Utf8Error> = item.to_str().map(|t| t.to_string());
        result.map_err(CxxConvertError::Utf8Error)
    }
}

// CxxVector -> Vec
pub struct CxxVectorStrategy<T>
where
    T: Clone + VectorElement,
{
    _marker: std::marker::PhantomData<T>,
}

impl<T> CxxVectorStrategy<T>
where
    T: Clone + VectorElement,
{
    pub fn new() -> Self {
        CxxVectorStrategy {
            _marker: std::marker::PhantomData,
        }
    }
}
impl<T> ConvertStrategy<CxxVector<T>, Vec<T>> for CxxVectorStrategy<T>
where
    T: Clone + VectorElement,
{
    fn convert(&self, items: &CxxVector<T>) -> Result<Vec<T>, CxxConvertError> {
        Ok(items.into_iter().map(|item| item.clone()).collect())
    }
}

// CxxVector<String> -> Vec<String>
pub struct CxxVectorStringStrategy;

impl ConvertStrategy<CxxVector<CxxString>, Vec<String>> for CxxVectorStringStrategy {
    fn convert(&self, items: &CxxVector<CxxString>) -> Result<Vec<String>, CxxConvertError> {
        items
            .iter()
            .map(|item| {
                item.to_str()
                    .map(|t| t.to_string())
                    .map_err(CxxConvertError::Utf8Error)
            })
            .collect()
    }
}

// CxxVector<String> -> Vec<u8>
pub struct CxxVectorStringToBytesStrategy;

impl ConvertStrategy<CxxVector<CxxString>, Vec<Vec<u8>>> for CxxVectorStringToBytesStrategy {
    fn convert(&self, items: &CxxVector<CxxString>) -> Result<Vec<Vec<u8>>, CxxConvertError> {
        items
            .iter()
            .map(|item| Ok(item.as_bytes().to_vec()))
            .collect()
    }
}

pub struct CxxVectorStringToDateTimeStrategy;

impl ConvertStrategy<CxxVector<CxxString>, Vec<DateTime>> for CxxVectorStringToDateTimeStrategy {
    fn convert(&self, items: &CxxVector<CxxString>) -> Result<Vec<DateTime>, CxxConvertError> {
        items
            .iter()
            .map(|item| {
                let str = match item.to_str() {
                    Ok(str) => str,
                    Err(e) => return Err(CxxConvertError::CxxVectorConvertError(e.to_string())),
                };
                match OffsetDateTime::parse(str, &Rfc3339) {
                    Ok(t) => {
                        return Ok(DateTime::from_utc(t));
                    }
                    Err(e) => {
                        return Err(CxxConvertError::CxxDateVectorConvertError(e.to_string()));
                    }
                }
            })
            .collect()
    }
}

pub struct Converter<T, U, S>
where
    S: ConvertStrategy<T, U>,
{
    strategy: S,
    _marker_t: std::marker::PhantomData<T>,
    _marker_u: std::marker::PhantomData<U>,
}

impl<T, U, S> Converter<T, U, S>
where
    S: ConvertStrategy<T, U>,
    // T: VectorElement,
{
    pub fn new(strategy: S) -> Self {
        Converter {
            strategy,
            _marker_t: std::marker::PhantomData,
            _marker_u: std::marker::PhantomData,
        }
    }

    pub fn convert(&self, item: &T) -> Result<U, CxxConvertError> {
        self.strategy.convert(item)
    }
}
