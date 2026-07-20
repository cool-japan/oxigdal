//! JPEG2000 progression order iterators (ISO 15444-1 Table A.16)
//!
//! JPEG2000 defines five packet progression orders that determine the order
//! in which packets (layer/resolution/component/precinct tuples) appear in
//! the codestream.

use crate::codestream::ProgressionOrder;

/// Address of a code block within the codestream, identified by the
/// four progression dimensions: layer, resolution, component, precinct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeBlockAddress {
    /// Quality layer index (0-based)
    pub layer: u16,
    /// Resolution level index (0-based)
    pub resolution: u8,
    /// Component index (0-based)
    pub component: u16,
    /// Precinct index within this resolution level (0-based)
    pub precinct: u32,
}

// ---------------------------------------------------------------------------
// Internal state machine for each of the 5 progression orders
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum ProgressionState {
    Lrcp {
        layer: u16,
        resolution: u8,
        component: u16,
        precinct: u32,
    },
    Rlcp {
        resolution: u8,
        layer: u16,
        component: u16,
        precinct: u32,
    },
    Rpcl {
        resolution: u8,
        precinct: u32,
        component: u16,
        layer: u16,
    },
    Pcrl {
        precinct: u32,
        component: u16,
        resolution: u8,
        layer: u16,
    },
    Cprl {
        component: u16,
        precinct: u32,
        resolution: u8,
        layer: u16,
    },
    Exhausted,
}

/// Iterator that yields [`CodeBlockAddress`] tuples in the order specified by
/// the selected [`ProgressionOrder`].
///
/// The iterator visits every valid (layer, resolution, component, precinct)
/// combination exactly once in the JPEG2000-specified order.
#[derive(Debug, Clone)]
pub struct ProgressionIterator {
    num_layers: u16,
    num_resolutions: u8,
    num_components: u16,
    /// Number of precincts per resolution level (index = resolution level)
    num_precincts: Vec<u32>,
    state: ProgressionState,
}

impl ProgressionIterator {
    /// Create a new [`ProgressionIterator`].
    ///
    /// # Parameters
    /// - `order`: Progression order as parsed from the COD marker.
    /// - `num_layers`: Number of quality layers.
    /// - `num_resolutions`: Number of resolution levels (including full resolution).
    /// - `num_components`: Number of image components.
    /// - `num_precincts`: Slice of precinct counts, one per resolution level.
    ///   If shorter than `num_resolutions`, the last entry is repeated.
    ///   Defaults to 1 precinct per level when empty.
    pub fn new(
        order: ProgressionOrder,
        num_layers: u16,
        num_resolutions: u8,
        num_components: u16,
        num_precincts: &[u32],
    ) -> Self {
        // Normalise precinct counts vector: length == num_resolutions
        let resolved_precincts: Vec<u32> = if num_precincts.is_empty() {
            vec![1; num_resolutions as usize]
        } else {
            let last = *num_precincts.last().unwrap_or(&1);
            let mut v: Vec<u32> = num_precincts
                .iter()
                .take(num_resolutions as usize)
                .copied()
                .collect();
            while v.len() < num_resolutions as usize {
                v.push(last);
            }
            v
        };

        let state = if num_layers == 0 || num_resolutions == 0 || num_components == 0 {
            ProgressionState::Exhausted
        } else {
            match order {
                ProgressionOrder::Lrcp => ProgressionState::Lrcp {
                    layer: 0,
                    resolution: 0,
                    component: 0,
                    precinct: 0,
                },
                ProgressionOrder::Rlcp => ProgressionState::Rlcp {
                    resolution: 0,
                    layer: 0,
                    component: 0,
                    precinct: 0,
                },
                ProgressionOrder::Rpcl => ProgressionState::Rpcl {
                    resolution: 0,
                    precinct: 0,
                    component: 0,
                    layer: 0,
                },
                ProgressionOrder::Pcrl => ProgressionState::Pcrl {
                    precinct: 0,
                    component: 0,
                    resolution: 0,
                    layer: 0,
                },
                ProgressionOrder::Cprl => ProgressionState::Cprl {
                    component: 0,
                    precinct: 0,
                    resolution: 0,
                    layer: 0,
                },
            }
        };

        Self {
            num_layers,
            num_resolutions,
            num_components,
            num_precincts: resolved_precincts,
            state,
        }
    }

    /// Return the total number of packets this iterator will yield.
    pub fn total_packets(&self) -> u64 {
        let precincts_total: u64 = self.num_precincts.iter().map(|&p| p as u64).sum();
        precincts_total * self.num_layers as u64 * self.num_components as u64
    }

    // -----------------------------------------------------------------------
    // Internal helpers that advance each dimension for each order
    // -----------------------------------------------------------------------

    fn precincts_for(&self, res: u8) -> u32 {
        self.num_precincts
            .get(res as usize)
            .copied()
            .unwrap_or(1)
            .max(1)
    }
}

impl Iterator for ProgressionIterator {
    type Item = CodeBlockAddress;

    fn next(&mut self) -> Option<Self::Item> {
        match &self.state {
            ProgressionState::Exhausted => None,

            // ----------------------------------------------------------
            // LRCP: Layer → Resolution → Component → Precinct (innermost)
            // ----------------------------------------------------------
            ProgressionState::Lrcp {
                layer,
                resolution,
                component,
                precinct,
            } => {
                let (l, r, c, p) = (*layer, *resolution, *component, *precinct);
                let item = CodeBlockAddress {
                    layer: l,
                    resolution: r,
                    component: c,
                    precinct: p,
                };

                // Advance innermost (precinct) first
                let max_p = self.precincts_for(r);
                let next_p = p + 1;
                let (nl, nr, nc, np) = if next_p < max_p {
                    (l, r, c, next_p)
                } else {
                    let next_c = c + 1;
                    if next_c < self.num_components {
                        (l, r, next_c, 0)
                    } else {
                        let next_r = r + 1;
                        if next_r < self.num_resolutions {
                            (l, next_r, 0, 0)
                        } else {
                            let next_l = l + 1;
                            if next_l < self.num_layers {
                                (next_l, 0, 0, 0)
                            } else {
                                self.state = ProgressionState::Exhausted;
                                return Some(item);
                            }
                        }
                    }
                };

                self.state = ProgressionState::Lrcp {
                    layer: nl,
                    resolution: nr,
                    component: nc,
                    precinct: np,
                };
                Some(item)
            }

            // ----------------------------------------------------------
            // RLCP: Resolution → Layer → Component → Precinct
            // ----------------------------------------------------------
            ProgressionState::Rlcp {
                resolution,
                layer,
                component,
                precinct,
            } => {
                let (r, l, c, p) = (*resolution, *layer, *component, *precinct);
                let item = CodeBlockAddress {
                    layer: l,
                    resolution: r,
                    component: c,
                    precinct: p,
                };

                let max_p = self.precincts_for(r);
                let (nr, nl, nc, np) = if p + 1 < max_p {
                    (r, l, c, p + 1)
                } else if c + 1 < self.num_components {
                    (r, l, c + 1, 0)
                } else if l + 1 < self.num_layers {
                    (r, l + 1, 0, 0)
                } else {
                    let next_r = r + 1;
                    if next_r < self.num_resolutions {
                        (next_r, 0, 0, 0)
                    } else {
                        self.state = ProgressionState::Exhausted;
                        return Some(item);
                    }
                };

                self.state = ProgressionState::Rlcp {
                    resolution: nr,
                    layer: nl,
                    component: nc,
                    precinct: np,
                };
                Some(item)
            }

            // ----------------------------------------------------------
            // RPCL: Resolution → Precinct → Component → Layer
            // ----------------------------------------------------------
            ProgressionState::Rpcl {
                resolution,
                precinct,
                component,
                layer,
            } => {
                let (r, p, c, l) = (*resolution, *precinct, *component, *layer);
                let item = CodeBlockAddress {
                    layer: l,
                    resolution: r,
                    component: c,
                    precinct: p,
                };

                let max_p = self.precincts_for(r);
                let (nr, np, nc, nl) = if l + 1 < self.num_layers {
                    (r, p, c, l + 1)
                } else if c + 1 < self.num_components {
                    (r, p, c + 1, 0)
                } else if p + 1 < max_p {
                    (r, p + 1, 0, 0)
                } else {
                    let next_r = r + 1;
                    if next_r < self.num_resolutions {
                        (next_r, 0, 0, 0)
                    } else {
                        self.state = ProgressionState::Exhausted;
                        return Some(item);
                    }
                };

                self.state = ProgressionState::Rpcl {
                    resolution: nr,
                    precinct: np,
                    component: nc,
                    layer: nl,
                };
                Some(item)
            }

            // ----------------------------------------------------------
            // PCRL: Precinct → Component → Resolution → Layer
            // ----------------------------------------------------------
            ProgressionState::Pcrl {
                precinct,
                component,
                resolution,
                layer,
            } => {
                let (p, c, r, l) = (*precinct, *component, *resolution, *layer);
                let item = CodeBlockAddress {
                    layer: l,
                    resolution: r,
                    component: c,
                    precinct: p,
                };

                let max_p = self.precincts_for(r);
                let (np, nc, nr, nl) = if l + 1 < self.num_layers {
                    (p, c, r, l + 1)
                } else if r + 1 < self.num_resolutions {
                    (p, c, r + 1, 0)
                } else if c + 1 < self.num_components {
                    (p, c + 1, 0, 0)
                } else if p + 1 < max_p {
                    (p + 1, 0, 0, 0)
                } else {
                    self.state = ProgressionState::Exhausted;
                    return Some(item);
                };

                self.state = ProgressionState::Pcrl {
                    precinct: np,
                    component: nc,
                    resolution: nr,
                    layer: nl,
                };
                Some(item)
            }

            // ----------------------------------------------------------
            // CPRL: Component → Precinct → Resolution → Layer
            // ----------------------------------------------------------
            ProgressionState::Cprl {
                component,
                precinct,
                resolution,
                layer,
            } => {
                let (c, p, r, l) = (*component, *precinct, *resolution, *layer);
                let item = CodeBlockAddress {
                    layer: l,
                    resolution: r,
                    component: c,
                    precinct: p,
                };

                let max_p = self.precincts_for(r);
                let (nc, np, nr, nl) = if l + 1 < self.num_layers {
                    (c, p, r, l + 1)
                } else if r + 1 < self.num_resolutions {
                    (c, p, r + 1, 0)
                } else if p + 1 < max_p {
                    (c, p + 1, 0, 0)
                } else {
                    let next_c = c + 1;
                    if next_c < self.num_components {
                        (next_c, 0, 0, 0)
                    } else {
                        self.state = ProgressionState::Exhausted;
                        return Some(item);
                    }
                };

                self.state = ProgressionState::Cprl {
                    component: nc,
                    precinct: np,
                    resolution: nr,
                    layer: nl,
                };
                Some(item)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codestream::ProgressionOrder;

    fn collect_all(iter: ProgressionIterator) -> Vec<CodeBlockAddress> {
        iter.collect()
    }

    #[test]
    fn test_lrcp_basic_order() {
        // 2 layers, 2 resolutions, 1 component, 1 precinct/level
        let iter = ProgressionIterator::new(ProgressionOrder::Lrcp, 2, 2, 1, &[1, 1]);
        let items = collect_all(iter);
        // Expected: (l=0,r=0), (l=0,r=1), (l=1,r=0), (l=1,r=1)
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].layer, 0);
        assert_eq!(items[0].resolution, 0);
        assert_eq!(items[1].layer, 0);
        assert_eq!(items[1].resolution, 1);
        assert_eq!(items[2].layer, 1);
        assert_eq!(items[2].resolution, 0);
        assert_eq!(items[3].layer, 1);
        assert_eq!(items[3].resolution, 1);
    }

    #[test]
    fn test_rlcp_basic_order() {
        let iter = ProgressionIterator::new(ProgressionOrder::Rlcp, 2, 2, 1, &[1, 1]);
        let items = collect_all(iter);
        // Expected: (r=0,l=0), (r=0,l=1), (r=1,l=0), (r=1,l=1)
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].resolution, 0);
        assert_eq!(items[0].layer, 0);
        assert_eq!(items[1].resolution, 0);
        assert_eq!(items[1].layer, 1);
        assert_eq!(items[2].resolution, 1);
        assert_eq!(items[2].layer, 0);
    }

    #[test]
    fn test_rpcl_basic_order() {
        // 1 layer, 2 resolutions, 2 components, 1 precinct/level
        let iter = ProgressionIterator::new(ProgressionOrder::Rpcl, 1, 2, 2, &[1, 1]);
        let items = collect_all(iter);
        // RPCL: r → p → c → l
        // (r=0,p=0,c=0,l=0), (r=0,p=0,c=1,l=0), (r=1,p=0,c=0,l=0), (r=1,p=0,c=1,l=0)
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].resolution, 0);
        assert_eq!(items[0].component, 0);
        assert_eq!(items[1].resolution, 0);
        assert_eq!(items[1].component, 1);
        assert_eq!(items[2].resolution, 1);
        assert_eq!(items[2].component, 0);
    }

    #[test]
    fn test_pcrl_basic_order() {
        // 1 layer, 1 resolution, 2 components, 2 precincts at res 0
        let iter = ProgressionIterator::new(ProgressionOrder::Pcrl, 1, 1, 2, &[2]);
        let items = collect_all(iter);
        // PCRL outermost = precinct, then component, then resolution, then layer
        // (p=0,c=0,r=0,l=0), (p=0,c=1,r=0,l=0), (p=1,c=0,r=0,l=0), (p=1,c=1,r=0,l=0)
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].precinct, 0);
        assert_eq!(items[0].component, 0);
        assert_eq!(items[1].precinct, 0);
        assert_eq!(items[1].component, 1);
        assert_eq!(items[2].precinct, 1);
        assert_eq!(items[2].component, 0);
    }

    #[test]
    fn test_cprl_basic_order() {
        // 1 layer, 2 resolutions, 2 components, 1 precinct
        let iter = ProgressionIterator::new(ProgressionOrder::Cprl, 1, 2, 2, &[1, 1]);
        let items = collect_all(iter);
        // CPRL: c → p → r → l
        // (c=0,p=0,r=0,l=0), (c=0,p=0,r=1,l=0), (c=1,p=0,r=0,l=0), (c=1,p=0,r=1,l=0)
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].component, 0);
        assert_eq!(items[0].resolution, 0);
        assert_eq!(items[1].component, 0);
        assert_eq!(items[1].resolution, 1);
        assert_eq!(items[2].component, 1);
        assert_eq!(items[2].resolution, 0);
    }

    #[test]
    fn test_empty_iterator_zero_layers() {
        let iter = ProgressionIterator::new(ProgressionOrder::Lrcp, 0, 3, 3, &[1, 1, 1]);
        let items: Vec<_> = iter.collect();
        assert!(items.is_empty());
    }

    #[test]
    fn test_single_item_iterator() {
        let iter = ProgressionIterator::new(ProgressionOrder::Lrcp, 1, 1, 1, &[1]);
        let items: Vec<_> = iter.collect();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].layer, 0);
        assert_eq!(items[0].resolution, 0);
        assert_eq!(items[0].component, 0);
        assert_eq!(items[0].precinct, 0);
    }

    #[test]
    fn test_multiple_precincts_per_resolution() {
        // 1 layer, 1 resolution, 1 component, 3 precincts
        let iter = ProgressionIterator::new(ProgressionOrder::Lrcp, 1, 1, 1, &[3]);
        let items: Vec<_> = iter.collect();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].precinct, 0);
        assert_eq!(items[1].precinct, 1);
        assert_eq!(items[2].precinct, 2);
    }

    #[test]
    fn test_total_packets_count() {
        // 3 layers, 2 resolutions (1 precinct each), 4 components
        // total = (1+1) * 3 * 4 = 24
        let iter = ProgressionIterator::new(ProgressionOrder::Lrcp, 3, 2, 4, &[1, 1]);
        assert_eq!(iter.total_packets(), 24);
    }

    #[test]
    fn test_default_precincts_when_empty_slice() {
        let iter = ProgressionIterator::new(ProgressionOrder::Lrcp, 1, 2, 1, &[]);
        let items: Vec<_> = iter.collect();
        // Should produce 2 items (1 precinct per resolution by default)
        assert_eq!(items.len(), 2);
    }
}
