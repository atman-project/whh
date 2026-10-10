use std::collections::HashMap;

use crate::{hlc::HybridLogicalClock, site::SiteId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionVector(HashMap<SiteId, HybridLogicalClock>);
