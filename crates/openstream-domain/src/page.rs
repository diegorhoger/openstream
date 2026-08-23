//! Pages: ordered grids of controls inside a deck (DOMAIN_MODEL.md §3).

use crate::control::{Control, ControlKind};
use crate::error::DomainError;
use crate::ids::{ControlId, DeckId, PageId};
use crate::limits::{MAX_CONTROLS_PER_PAGE, check_text};
use serde::{Deserialize, Serialize};

/// Grid dimensions of a page; both axes are at least one cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridDimensions {
    /// Columns; at least one.
    pub columns: u16,
    /// Rows; at least one.
    pub rows: u16,
}

impl GridDimensions {
    /// Validates and constructs (zero on either axis rejects).
    pub const fn new(columns: u16, rows: u16) -> Result<Self, DomainError> {
        if columns == 0 || rows == 0 {
            return Err(DomainError::ZeroGridDimension);
        }
        Ok(Self { columns, rows })
    }

    /// Total number of cells.
    #[must_use]
    pub const fn cells(&self) -> usize {
        self.columns as usize * self.rows as usize
    }
}

/// One page of a deck with its controls.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    /// Durable identifier (UUIDv7).
    pub id: PageId,
    /// Owning deck.
    pub deck_id: DeckId,
    /// Ordinal inside the deck; unique per deck.
    pub ordinal: u32,
    /// Grid geometry (page-relative coordinates).
    pub grid: GridDimensions,
    /// Controls placed on this page.
    pub controls: Vec<Control>,
}

impl Page {
    /// Full structural validation of the page subtree (save-time S2-style
    /// checks for this entity, DOMAIN_MODEL.md §6): grid dimensions exist,
    /// control count within limit, identifiers unique, controls belong to
    /// this page, labels valid, interaction policy allowed by kind, and
    /// geometry inside the grid.
    pub fn validate(&self) -> Result<(), DomainError> {
        GridDimensions::new(self.grid.columns, self.grid.rows)?;
        if self.controls.len() > MAX_CONTROLS_PER_PAGE {
            return Err(DomainError::LimitExceeded {
                what: "controls per page",
                limit: MAX_CONTROLS_PER_PAGE,
            });
        }
        for (index, control) in self.controls.iter().enumerate() {
            if control.page_id != self.id {
                return Err(DomainError::ForeignControlPage);
            }
            if self.controls[..index]
                .iter()
                .any(|other| other.id == control.id)
            {
                return Err(DomainError::DuplicateControlId);
            }
            check_text("label", &control.label)?;
            // A state sink must carry no interaction policy; interactive
            // kinds must carry one their kind admits (fail closed).
            let policy_valid = match (control.kind, control.policy) {
                (ControlKind::VariableDisplay, None) => true,
                (_, Some(policy)) => control.kind.allows(&policy),
                (_, None) => false,
            };
            if !policy_valid {
                return Err(DomainError::PolicyNotAllowedForKind);
            }
            let geometry = control.geometry;
            if geometry.width == 0 || geometry.height == 0 {
                return Err(DomainError::ZeroGeometryExtent);
            }
            let max_x = u32::from(geometry.x) + u32::from(geometry.width);
            let max_y = u32::from(geometry.y) + u32::from(geometry.height);
            if max_x > u32::from(self.grid.columns) {
                return Err(DomainError::GeometryOutsideGrid { axis: "x" });
            }
            if max_y > u32::from(self.grid.rows) {
                return Err(DomainError::GeometryOutsideGrid { axis: "y" });
            }
        }
        Ok(())
    }

    /// Deterministically-ordered pairs of overlapping control rectangles.
    ///
    /// Collisions are reported, not rejected: sync merges preserve both edits
    /// and mark `needs_resolution` (TECHNICAL_SPEC §6). Pairs are ordered by
    /// position in [`Self::controls`] and each pair lists the earlier control
    /// first.
    #[must_use]
    pub fn grid_collisions(&self) -> Vec<(ControlId, ControlId)> {
        let mut pairs = Vec::new();
        for (i, a) in self.controls.iter().enumerate() {
            for b in &self.controls[i + 1..] {
                let (ga, gb) = (&a.geometry, &b.geometry);
                let overlap_x = u32::from(ga.x) < u32::from(gb.x) + u32::from(gb.width)
                    && u32::from(gb.x) < u32::from(ga.x) + u32::from(ga.width);
                let overlap_y = u32::from(ga.y) < u32::from(gb.y) + u32::from(gb.height)
                    && u32::from(gb.y) < u32::from(ga.y) + u32::from(ga.height);
                if overlap_x && overlap_y {
                    pairs.push((a.id, b.id));
                }
            }
        }
        pairs
    }
}

#[cfg(test)]
mod tests {
    use super::{GridDimensions, Page};
    use crate::control::{Control, ControlKind, Geometry, InteractionPolicy};
    use crate::error::DomainError;
    use crate::ids::{ControlId, DeckId, PageId};
    use crate::limits::MAX_CONTROLS_PER_PAGE;
    use std::str::FromStr as _;

    fn uuid7(n: u32) -> String {
        format!("018f6a1c-7b21-7{n:03x}-9f31-{n:012x}")
    }

    fn deck() -> DeckId {
        DeckId::from_str(&uuid7(1)).unwrap()
    }

    fn page(deck_id: DeckId) -> Page {
        Page {
            id: PageId::from_str(&uuid7(2)).unwrap(),
            deck_id,
            ordinal: 0,
            grid: GridDimensions {
                columns: 8,
                rows: 4,
            },
            controls: Vec::new(),
        }
    }

    fn control(page_id: PageId, geometry: Geometry) -> Control {
        Control {
            id: ControlId::generate(),
            page_id,
            kind: ControlKind::Button,
            geometry,
            label: "mute mic".into(),
            policy: Some(InteractionPolicy::Press),
            enabled: true,
        }
    }

    #[test]
    fn zero_grid_dimension_rejects() {
        let mut p = page(deck());
        p.grid = GridDimensions {
            columns: 0,
            rows: 4,
        };
        assert_eq!(p.validate(), Err(DomainError::ZeroGridDimension));
        p.grid = GridDimensions {
            columns: 8,
            rows: 0,
        };
        assert_eq!(p.validate(), Err(DomainError::ZeroGridDimension));
    }

    #[test]
    fn grid_dimensions_new_validates() {
        assert!(GridDimensions::new(1, 1).is_ok());
        assert_eq!(
            GridDimensions::new(0, 1),
            Err(DomainError::ZeroGridDimension)
        );
        assert_eq!(GridDimensions::new(2, 2).unwrap().cells(), 4);
    }

    #[test]
    fn foreign_control_page_rejects() {
        let d = deck();
        let mut p = page(d);
        let other = PageId::from_str(&uuid7(3)).unwrap();
        p.controls.push(control(
            other,
            Geometry {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        ));
        assert_eq!(p.validate(), Err(DomainError::ForeignControlPage));
    }

    #[test]
    fn duplicate_control_ids_reject() {
        let mut p = page(deck());
        let shared = ControlId::generate();
        let mut a = control(
            p.id,
            Geometry {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        );
        a.id = shared;
        let mut b = control(
            p.id,
            Geometry {
                x: 5,
                y: 5,
                width: 1,
                height: 1,
            },
        );
        b.id = shared;
        p.controls.extend([a, b]);
        assert_eq!(p.validate(), Err(DomainError::DuplicateControlId));
    }

    #[test]
    fn geometry_outside_grid_rejects_per_axis() {
        let mut p = page(deck());
        // x overflow: 7 + 2 > 8 columns.
        p.controls.push(control(
            p.id,
            Geometry {
                x: 7,
                y: 0,
                width: 2,
                height: 1,
            },
        ));
        assert_eq!(
            p.validate(),
            Err(DomainError::GeometryOutsideGrid { axis: "x" })
        );
        p.controls.clear();
        // y overflow: 3 + 2 > 4 rows.
        p.controls.push(control(
            p.id,
            Geometry {
                x: 0,
                y: 3,
                width: 1,
                height: 2,
            },
        ));
        assert_eq!(
            p.validate(),
            Err(DomainError::GeometryOutsideGrid { axis: "y" })
        );
        p.controls.clear();
        // Exact fit passes.
        p.controls.push(control(
            p.id,
            Geometry {
                x: 7,
                y: 3,
                width: 1,
                height: 1,
            },
        ));
        assert_eq!(p.validate(), Ok(()));
    }

    #[test]
    fn zero_geometry_extent_rejects() {
        let mut p = page(deck());
        p.controls.push(control(
            p.id,
            Geometry {
                x: 0,
                y: 0,
                width: 0,
                height: 1,
            },
        ));
        assert_eq!(p.validate(), Err(DomainError::ZeroGeometryExtent));
    }

    #[test]
    fn empty_label_rejects_and_policy_matrix_enforced_on_controls() {
        let mut p = page(deck());
        let mut unlabeled = control(
            p.id,
            Geometry {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        );
        unlabeled.label = "  ".into();
        p.controls.push(unlabeled);
        assert_eq!(
            p.validate(),
            Err(DomainError::TextFieldOutOfRange { field: "label" })
        );

        let mut p = page(deck());
        // Button requires a policy (fail closed on missing semantics).
        let mut policyless = control(
            p.id,
            Geometry {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        );
        policyless.policy = None;
        p.controls.push(policyless);
        assert_eq!(p.validate(), Err(DomainError::PolicyNotAllowedForKind));

        let mut p = page(deck());
        // Toggle kind cannot carry hold.
        let mut wrong = control(
            p.id,
            Geometry {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        );
        wrong.kind = ControlKind::Toggle;
        wrong.policy = Some(InteractionPolicy::Hold);
        p.controls.push(wrong);
        assert_eq!(p.validate(), Err(DomainError::PolicyNotAllowedForKind));

        let mut p = page(deck());
        // Variable display is a state sink: no policy allowed.
        let mut sink = control(
            p.id,
            Geometry {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        );
        sink.kind = ControlKind::VariableDisplay;
        sink.policy = Some(InteractionPolicy::Press);
        p.controls.push(sink);
        assert_eq!(p.validate(), Err(DomainError::PolicyNotAllowedForKind));

        let mut p = page(deck());
        let sink = Control {
            kind: ControlKind::VariableDisplay,
            policy: None,
            ..control(
                p.id,
                Geometry {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
            )
        };
        p.controls.push(sink);
        assert_eq!(p.validate(), Ok(()));
    }

    #[test]
    fn control_limit_enforced() {
        let mut p = page(deck());
        for _ in 0..MAX_CONTROLS_PER_PAGE + 1 {
            p.controls.push(control(
                p.id,
                Geometry {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
            ));
        }
        match p.validate() {
            Err(DomainError::LimitExceeded { what, limit }) => {
                assert_eq!(what, "controls per page");
                assert_eq!(limit, MAX_CONTROLS_PER_PAGE);
            }
            other => panic!("expected LimitExceeded, got {other:?}"),
        }
    }

    #[test]
    fn collisions_reported_not_rejected() {
        let mut p = page(deck());
        let a_id = ControlId::from_str(&uuid7(4)).unwrap();
        let b_id = ControlId::from_str(&uuid7(5)).unwrap();
        let mut a = control(
            p.id,
            Geometry {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            },
        );
        a.id = a_id;
        let mut b = control(
            p.id,
            Geometry {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            },
        );
        b.id = b_id;
        let c = control(
            p.id,
            Geometry {
                x: 5,
                y: 0,
                width: 1,
                height: 1,
            },
        );
        p.controls.extend([a, b, c]);
        assert_eq!(p.validate(), Ok(()));
        assert_eq!(p.grid_collisions(), vec![(a_id, b_id)]);
    }
}
