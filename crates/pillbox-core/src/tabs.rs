//! The card's tabs: which there are, in what order the card shows them, and which are hidden.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tab {
    Home,
    Music,
    Clipboard,
    Shelf,
    Reminders,
    Controls,
    /// CPU, GPU, RAM, disk, network and battery, one card each, on a wide card.
    System,
    /// M7: coding agents and live statuses from scripts.
    Activity,
    /// M8: Windows toast notifications mirrored on Pillbox.
    Notifications,
}

impl Tab {
    /// Every tab, in the default order.
    pub const ALL: [Tab; 9] = [
        Tab::Home,
        Tab::Music,
        Tab::Clipboard,
        Tab::Shelf,
        Tab::Reminders,
        Tab::Controls,
        Tab::System,
        Tab::Activity,
        Tab::Notifications,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tab::Home => "Home",
            Tab::Music => "Music",
            Tab::Clipboard => "Clipboard",
            Tab::Shelf => "Shelf",
            Tab::Reminders => "Reminders",
            Tab::Controls => "Controls",
            Tab::Activity => "Activity",
            Tab::Notifications => "Notifications",
            Tab::System => "System",
        }
    }

    /// Segoe Fluent Icons glyph.
    pub fn icon(self) -> &'static str {
        match self {
            Tab::Home => "\u{E80F}",
            Tab::Music => "\u{E8D6}",
            Tab::Clipboard => "\u{E77F}",
            Tab::Shelf => "\u{E7B8}",
            Tab::Reminders => "\u{EA8F}",
            Tab::Controls => "\u{E9E9}",
            Tab::Activity => "\u{E9D9}",
            Tab::Notifications => "\u{E8BD}",
            Tab::System => "\u{E950}",
        }
    }

    /// List tabs use the taller card.
    pub fn tall(self) -> bool {
        !matches!(self, Tab::Music | Tab::System)
    }

    /// Home always stays: it is where the card falls back to.
    pub fn can_hide(self) -> bool {
        self != Tab::Home
    }
}

/// The user's order of the tabs and the ones left out of the card. Hiding a tab does not
/// switch its feature off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TabsSettings {
    pub order: Vec<Tab>,
    pub hidden: Vec<Tab>,
}

impl Default for TabsSettings {
    fn default() -> Self {
        Self {
            order: Tab::ALL.to_vec(),
            hidden: Vec::new(),
        }
    }
}

impl TabsSettings {
    /// Every tab once: repeats are dropped, missing ones go to the end in the default order
    /// (so a file from an older version also gets tabs added later).
    pub fn sanitize(&mut self) {
        let mut order: Vec<Tab> = Vec::with_capacity(Tab::ALL.len());
        for tab in self.order.iter().chain(&Tab::ALL) {
            if !order.contains(tab) {
                order.push(*tab);
            }
        }
        self.order = order;
        self.hidden.retain(|t| t.can_hide());
        self.hidden.sort_by_key(|t| *t as u8);
        self.hidden.dedup();
    }

    pub fn is_hidden(&self, tab: Tab) -> bool {
        self.hidden.contains(&tab)
    }

    pub fn set_hidden(&mut self, tab: Tab, hidden: bool) {
        self.hidden.retain(|t| *t != tab);
        if hidden && tab.can_hide() {
            self.hidden.push(tab);
            self.hidden.sort_by_key(|t| *t as u8);
        }
    }

    /// The tabs the card shows: in the user's order, without the hidden ones and without those
    /// whose feature is off. `summoned` is a hidden tab something asked for (files dropped on
    /// the pill, a reminder ringing): it shows until the card is opened again.
    pub fn shown(&self, available: impl Fn(Tab) -> bool, summoned: Option<Tab>) -> Vec<Tab> {
        self.order
            .iter()
            .copied()
            .filter(|t| available(*t) && (!self.is_hidden(*t) || summoned == Some(*t)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_shows_every_available_tab_in_order() {
        let s = TabsSettings::default();
        assert_eq!(s.shown(|_| true, None), Tab::ALL);
        let some = s.shown(|t| t != Tab::Music && t != Tab::Activity, None);
        assert_eq!(some.len(), 7);
        assert!(!some.contains(&Tab::Music) && !some.contains(&Tab::Activity));
    }

    #[test]
    fn hidden_tabs_go_unless_summoned_and_home_stays() {
        let mut s = TabsSettings::default();
        s.set_hidden(Tab::Shelf, true);
        s.set_hidden(Tab::Home, true);
        assert!(s.is_hidden(Tab::Shelf) && !s.is_hidden(Tab::Home));
        assert!(!s.shown(|_| true, None).contains(&Tab::Shelf));
        // It comes back to its own place, not to the end.
        assert_eq!(s.shown(|_| true, Some(Tab::Shelf)), Tab::ALL);
        // A summoned tab whose feature is off still does not show.
        assert!(!s.shown(|_| false, Some(Tab::Shelf)).contains(&Tab::Shelf));
        s.set_hidden(Tab::Shelf, false);
        assert_eq!(s, TabsSettings::default());
    }

    #[test]
    fn a_hand_edited_order_is_completed() {
        let mut s = TabsSettings {
            order: vec![Tab::Music, Tab::Home, Tab::Music],
            hidden: vec![Tab::System, Tab::Home, Tab::System],
        };
        s.sanitize();
        assert_eq!(s.order[..3], [Tab::Music, Tab::Home, Tab::Clipboard]);
        assert_eq!(s.order.len(), Tab::ALL.len());
        assert_eq!(s.hidden, [Tab::System]);
    }
}
