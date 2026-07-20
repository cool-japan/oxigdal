//! Basic transformation operations.

use crate::core::stream::{StreamElement, StreamMessage};
use std::sync::Arc;

/// Map transformation.
pub struct MapTransform<F>
where
    F: Fn(Vec<u8>) -> Vec<u8> + Send + Sync,
{
    mapper: Arc<F>,
}

impl<F> MapTransform<F>
where
    F: Fn(Vec<u8>) -> Vec<u8> + Send + Sync,
{
    /// Create a new map transformation.
    pub fn new(mapper: F) -> Self {
        Self {
            mapper: Arc::new(mapper),
        }
    }

    /// Apply the transformation to an element.
    pub fn apply(&self, element: StreamElement) -> StreamElement {
        let new_data = (self.mapper)(element.data);
        StreamElement {
            data: new_data,
            event_time: element.event_time,
            processing_time: element.processing_time,
            key: element.key,
            metadata: element.metadata,
        }
    }

    /// Apply the transformation to a message.
    pub fn apply_message(&self, message: StreamMessage) -> StreamMessage {
        match message {
            StreamMessage::Data(elem) => StreamMessage::Data(self.apply(elem)),
            other => other,
        }
    }
}

/// Filter transformation.
pub struct FilterTransform<F>
where
    F: Fn(&StreamElement) -> bool + Send + Sync,
{
    predicate: Arc<F>,
}

impl<F> FilterTransform<F>
where
    F: Fn(&StreamElement) -> bool + Send + Sync,
{
    /// Create a new filter transformation.
    pub fn new(predicate: F) -> Self {
        Self {
            predicate: Arc::new(predicate),
        }
    }

    /// Check if an element passes the filter.
    pub fn test(&self, element: &StreamElement) -> bool {
        (self.predicate)(element)
    }

    /// Apply the filter to a message.
    pub fn apply_message(&self, message: StreamMessage) -> Option<StreamMessage> {
        match message {
            StreamMessage::Data(elem) => {
                if self.test(&elem) {
                    Some(StreamMessage::Data(elem))
                } else {
                    None
                }
            }
            other => Some(other),
        }
    }
}

/// FlatMap transformation.
pub struct FlatMapTransform<F>
where
    F: Fn(Vec<u8>) -> Vec<Vec<u8>> + Send + Sync,
{
    mapper: Arc<F>,
}

impl<F> FlatMapTransform<F>
where
    F: Fn(Vec<u8>) -> Vec<Vec<u8>> + Send + Sync,
{
    /// Create a new flat map transformation.
    pub fn new(mapper: F) -> Self {
        Self {
            mapper: Arc::new(mapper),
        }
    }

    /// Apply the transformation to an element.
    pub fn apply(&self, element: StreamElement) -> Vec<StreamElement> {
        let new_data_vec = (self.mapper)(element.data);
        new_data_vec
            .into_iter()
            .map(|data| StreamElement {
                data,
                event_time: element.event_time,
                processing_time: element.processing_time,
                key: element.key.clone(),
                metadata: element.metadata.clone(),
            })
            .collect()
    }

    /// Apply the transformation to a message.
    pub fn apply_message(&self, message: StreamMessage) -> Vec<StreamMessage> {
        match message {
            StreamMessage::Data(elem) => self
                .apply(elem)
                .into_iter()
                .map(StreamMessage::Data)
                .collect(),
            other => vec![other],
        }
    }
}

/// KeyBy transformation.
pub struct KeyByTransform<F>
where
    F: Fn(&Vec<u8>) -> Vec<u8> + Send + Sync,
{
    key_selector: Arc<F>,
}

impl<F> KeyByTransform<F>
where
    F: Fn(&Vec<u8>) -> Vec<u8> + Send + Sync,
{
    /// Create a new keyBy transformation.
    pub fn new(key_selector: F) -> Self {
        Self {
            key_selector: Arc::new(key_selector),
        }
    }

    /// Apply the transformation to an element.
    pub fn apply(&self, element: StreamElement) -> StreamElement {
        let key = (self.key_selector)(&element.data);
        StreamElement {
            data: element.data,
            event_time: element.event_time,
            processing_time: element.processing_time,
            key: Some(key),
            metadata: element.metadata,
        }
    }

    /// Apply the transformation to a message.
    pub fn apply_message(&self, message: StreamMessage) -> StreamMessage {
        match message {
            StreamMessage::Data(elem) => StreamMessage::Data(self.apply(elem)),
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_map_transform() {
        let transform = MapTransform::new(|mut data: Vec<u8>| {
            data.push(99);
            data
        });

        let elem = StreamElement::new(vec![1, 2, 3], Utc::now());
        let result = transform.apply(elem);

        assert_eq!(result.data, vec![1, 2, 3, 99]);
    }

    #[test]
    fn test_filter_transform() {
        let transform = FilterTransform::new(|elem: &StreamElement| elem.data.len() > 2);

        let elem1 = StreamElement::new(vec![1, 2, 3], Utc::now());
        let elem2 = StreamElement::new(vec![1], Utc::now());

        assert!(transform.test(&elem1));
        assert!(!transform.test(&elem2));
    }

    #[test]
    fn test_flatmap_transform() {
        let transform = FlatMapTransform::new(|data: Vec<u8>| vec![data.clone(), data]);

        let elem = StreamElement::new(vec![1, 2, 3], Utc::now());
        let result = transform.apply(elem);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].data, vec![1, 2, 3]);
        assert_eq!(result[1].data, vec![1, 2, 3]);
    }

    #[test]
    fn test_keyby_transform() {
        let transform = KeyByTransform::new(|data: &Vec<u8>| vec![data.len() as u8]);

        let elem = StreamElement::new(vec![1, 2, 3], Utc::now());
        let result = transform.apply(elem);

        assert_eq!(result.key, Some(vec![3]));
    }
}
