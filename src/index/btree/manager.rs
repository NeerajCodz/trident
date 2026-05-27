use super::page::{BTreePage, BTreePageId};
use crate::errors::{PraxisError, Result};
use crate::store::RecordId;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BTreePageSplit {
    pub left: BTreePageId,
    pub right: BTreePageId,
    pub fence_key: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct BTreePageManager {
    next_page_id: u64,
    pages: BTreeMap<u64, BTreePage>,
}

impl BTreePageManager {
    pub fn new() -> Self {
        Self {
            next_page_id: 1,
            pages: BTreeMap::new(),
        }
    }

    pub fn allocate_leaf(&mut self, page_lsn: u64) -> BTreePageId {
        let page_id = BTreePageId(self.next_page_id);
        self.next_page_id = self.next_page_id.saturating_add(1);
        self.pages
            .insert(page_id.0, BTreePage::leaf(page_id, page_lsn));
        page_id
    }

    pub fn insert_leaf(
        &mut self,
        page_id: BTreePageId,
        key: impl Into<Vec<u8>>,
        sequence: u64,
        rid: RecordId,
    ) -> Result<()> {
        let page = self.page_mut(page_id)?;
        page.insert(key, sequence, rid);
        Ok(())
    }

    pub fn page(&self, page_id: BTreePageId) -> Result<&BTreePage> {
        self.pages.get(&page_id.0).ok_or(PraxisError::KeyNotFound)
    }

    pub fn page_mut(&mut self, page_id: BTreePageId) -> Result<&mut BTreePage> {
        self.pages
            .get_mut(&page_id.0)
            .ok_or(PraxisError::KeyNotFound)
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn split_leaf(&mut self, page_id: BTreePageId, page_lsn: u64) -> Result<BTreePageSplit> {
        let original = self.page(page_id)?.clone();
        let midpoint = original.entries().len() / 2;
        if midpoint == 0 {
            return Err(PraxisError::InvalidConfig(
                "cannot split an empty B-tree page".to_string(),
            ));
        }

        let right_id = self.allocate_leaf(page_lsn);
        let right_entries = original.entries()[midpoint..].to_vec();
        let left_entries = original.entries()[..midpoint].to_vec();

        {
            let left = self.page_mut(page_id)?;
            *left = BTreePage::leaf(page_id, page_lsn);
            left.right_sibling = Some(right_id);
            for entry in left_entries {
                left.insert(entry.key, entry.sequence, entry.rid);
            }
        }
        {
            let right = self.page_mut(right_id)?;
            right.right_sibling = original.right_sibling;
            for entry in &right_entries {
                right.insert(entry.key.clone(), entry.sequence, entry.rid);
            }
        }

        Ok(BTreePageSplit {
            left: page_id,
            right: right_id,
            fence_key: right_entries
                .first()
                .map(|entry| entry.key.clone())
                .unwrap_or_default(),
        })
    }
}
