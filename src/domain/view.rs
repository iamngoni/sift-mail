//! The inbox views shown as tabs in the title bar.

/// A built-in inbox view. Order here is tab order and the 1-6 shortcut order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TriageView {
    /// Everything that is not done or snoozed.
    #[default]
    All,
    /// People waiting on a reply or decision.
    NeedsYou,
    /// Invites and calendar changes.
    Meetings,
    /// Notifications and receipts.
    Updates,
    /// Newsletters and digests.
    Newsletters,
    /// Anything that fits nowhere else.
    Other,
}

impl TriageView {
    /// Every view, in tab order.
    pub const ALL: [TriageView; 6] = [
        TriageView::All,
        TriageView::NeedsYou,
        TriageView::Meetings,
        TriageView::Updates,
        TriageView::Newsletters,
        TriageView::Other,
    ];

    /// Tab label.
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::NeedsYou => "Needs you",
            Self::Meetings => "Meetings",
            Self::Updates => "Updates",
            Self::Newsletters => "Newsletters",
            Self::Other => "Other",
        }
    }

    /// Stable id for UI elements and tests, such as `view-needs-you`.
    pub fn id(self) -> &'static str {
        match self {
            Self::All => "view-all",
            Self::NeedsYou => "view-needs-you",
            Self::Meetings => "view-meetings",
            Self::Updates => "view-updates",
            Self::Newsletters => "view-newsletters",
            Self::Other => "view-other",
        }
    }

    /// The view bound to a number key, `1` for All through `6` for Other.
    pub fn from_shortcut(digit: u8) -> Option<Self> {
        let index = usize::from(digit).checked_sub(1)?;
        Self::ALL.get(index).copied()
    }

    /// The number key bound to this view.
    pub fn shortcut(self) -> u8 {
        let index = Self::ALL
            .iter()
            .position(|v| *v == self)
            .unwrap_or_default();
        // ALL has six entries, so this always fits in a u8.
        u8::try_from(index + 1).unwrap_or(u8::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_round_trip() {
        for view in TriageView::ALL {
            assert_eq!(TriageView::from_shortcut(view.shortcut()), Some(view));
        }
        assert_eq!(TriageView::from_shortcut(0), None);
        assert_eq!(TriageView::from_shortcut(7), None);
        assert_eq!(TriageView::default(), TriageView::All);
    }

    #[test]
    fn labels_and_ids_are_distinct() {
        let mut labels: Vec<_> = TriageView::ALL.iter().map(|v| v.label()).collect();
        let mut ids: Vec<_> = TriageView::ALL.iter().map(|v| v.id()).collect();
        labels.dedup();
        ids.dedup();
        assert_eq!(labels.len(), 6);
        assert_eq!(ids.len(), 6);
        assert!(ids.iter().all(|id| id.starts_with("view-")));
        assert_eq!(TriageView::NeedsYou.label(), "Needs you");
    }
}
