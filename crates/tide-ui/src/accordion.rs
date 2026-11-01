use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use tide_core::num::saturating_u16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionId {
    Explorer,
    SourceControl,
    Outline,
    Timeline,
}

impl SectionId {
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Explorer => "explorer",
            Self::SourceControl => "source-control",
            Self::Outline => "outline",
            Self::Timeline => "timeline",
        }
    }

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Explorer => "Explorer",
            Self::SourceControl => "Source Control",
            Self::Outline => "Outline",
            Self::Timeline => "Timeline",
        }
    }

    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "explorer" | "files" => Some(Self::Explorer),
            "source-control" | "git" => Some(Self::SourceControl),
            "outline" => Some(Self::Outline),
            "timeline" => Some(Self::Timeline),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AccordionSection {
    pub id: SectionId,
    pub status: Option<String>,
    pub expanded: bool,
}

#[derive(Debug, Clone)]
pub struct AccordionState {
    pub sections: Vec<AccordionSection>,
    pub focused: usize,
    pub content_focus: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionLayout {
    pub index: usize,
    pub id: SectionId,
    pub header: Rect,
    pub content: Rect,
    pub expanded: bool,
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccordionLayout {
    pub sections: Vec<SectionLayout>,
    pub compact: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccordionAction {
    None,
    Focus(usize),
    Toggle(usize),
    Expand(usize),
    Collapse(usize),
    EnterContent(usize),
    LeaveContent,
    Blur,
}

impl Default for AccordionState {
    fn default() -> Self {
        Self::new()
    }
}

impl AccordionState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            sections: vec![
                AccordionSection {
                    id: SectionId::Explorer,
                    status: None,
                    expanded: true,
                },
                AccordionSection {
                    id: SectionId::SourceControl,
                    status: None,
                    expanded: true,
                },
                AccordionSection {
                    id: SectionId::Outline,
                    status: None,
                    expanded: false,
                },
                AccordionSection {
                    id: SectionId::Timeline,
                    status: None,
                    expanded: false,
                },
            ],
            focused: 0,
            content_focus: false,
        }
    }

    #[must_use]
    pub fn focused_section(&self) -> SectionId {
        self.sections
            .get(self.focused)
            .map_or(SectionId::Explorer, |section| section.id)
    }

    #[must_use]
    pub fn expanded_ids(&self) -> Vec<String> {
        self.sections
            .iter()
            .filter(|section| section.expanded)
            .map(|section| section.id.id().to_owned())
            .collect()
    }

    pub fn set_expanded_ids(&mut self, ids: &[String]) {
        for section in &mut self.sections {
            section.expanded = ids
                .iter()
                .any(|id| SectionId::from_id(id) == Some(section.id));
        }
    }

    pub fn add(&mut self, id: SectionId) {
        if let Some(index) = self.sections.iter().position(|section| section.id == id) {
            self.focused = index;
            self.sections[index].expanded = true;
        } else {
            self.sections.push(AccordionSection {
                id,
                status: None,
                expanded: true,
            });
            self.focused = self.sections.len() - 1;
        }
        self.content_focus = false;
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.sections.len() {
            self.sections.remove(index);
            self.focused = self.focused.min(self.sections.len().saturating_sub(1));
            self.content_focus = false;
        }
    }

    pub fn set_status(&mut self, id: SectionId, status: Option<String>) {
        if let Some(section) = self.sections.iter_mut().find(|section| section.id == id) {
            section.status = status;
        }
    }

    #[must_use]
    pub fn layout(&self, area: Rect) -> AccordionLayout {
        let count = self.sections.len();
        if count == 0 {
            return AccordionLayout {
                sections: Vec::new(),
                compact: false,
            };
        }
        let compact = (area.height as usize) < count;
        let mut sections = Vec::with_capacity(count);
        if compact {
            let visible = area.height as usize;
            let start = self
                .focused
                .saturating_sub(visible.saturating_sub(1))
                .min(count.saturating_sub(1));
            for index in start..(start + visible).min(count) {
                let section = &self.sections[index];
                sections.push(SectionLayout {
                    index,
                    id: section.id,
                    header: Rect::new(
                        area.x,
                        area.y + saturating_u16(index - start),
                        area.width,
                        1,
                    ),
                    content: Rect::new(area.x, area.y, 0, 0),
                    expanded: false,
                    focused: index == self.focused,
                });
            }
            return AccordionLayout {
                sections,
                compact: true,
            };
        }

        let total_content = usize::from(area.height.saturating_sub(saturating_u16(count)));
        let expanded_count = self
            .sections
            .iter()
            .filter(|section| section.expanded)
            .count()
            .max(1);
        let base_content = total_content / expanded_count;
        let remainder = total_content % expanded_count;
        let mut y = area.y;
        let mut expanded_index = 0;
        for (index, section) in self.sections.iter().enumerate() {
            let content_height = if section.expanded {
                let extra = usize::from(expanded_index < remainder);
                expanded_index += 1;
                saturating_u16(base_content + extra)
            } else {
                0
            };
            let header = Rect::new(area.x, y, area.width, 1);
            y = y.saturating_add(1);
            let content = Rect::new(area.x, y, area.width, content_height);
            y = y.saturating_add(content_height);
            sections.push(SectionLayout {
                index,
                id: section.id,
                header,
                content,
                expanded: section.expanded,
                focused: index == self.focused,
            });
        }
        AccordionLayout {
            sections,
            compact: false,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> AccordionAction {
        if self.sections.is_empty() {
            return AccordionAction::None;
        }
        if key.modifiers.intersects(
            KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::META,
        ) {
            return AccordionAction::None;
        }
        match key.code {
            KeyCode::Esc => {
                if self.content_focus {
                    self.content_focus = false;
                    AccordionAction::LeaveContent
                } else {
                    AccordionAction::Blur
                }
            }
            KeyCode::Up | KeyCode::Char('k') if !self.content_focus => {
                self.focused = self.focused.saturating_sub(1);
                AccordionAction::Focus(self.focused)
            }
            KeyCode::Down | KeyCode::Char('j') if !self.content_focus => {
                self.focused = (self.focused + 1).min(self.sections.len() - 1);
                AccordionAction::Focus(self.focused)
            }
            KeyCode::Home if !self.content_focus => {
                self.focused = 0;
                AccordionAction::Focus(0)
            }
            KeyCode::End if !self.content_focus => {
                self.focused = self.sections.len() - 1;
                AccordionAction::Focus(self.focused)
            }
            KeyCode::Enter | KeyCode::Char(' ') if !self.content_focus => {
                let section = &mut self.sections[self.focused];
                section.expanded = !section.expanded;
                AccordionAction::Toggle(self.focused)
            }
            KeyCode::Right if !self.content_focus => {
                if !self.sections[self.focused].expanded {
                    self.sections[self.focused].expanded = true;
                    AccordionAction::Expand(self.focused)
                } else {
                    self.content_focus = true;
                    AccordionAction::EnterContent(self.focused)
                }
            }
            KeyCode::Left if !self.content_focus => {
                let section = &mut self.sections[self.focused];
                if section.expanded {
                    section.expanded = false;
                    AccordionAction::Collapse(self.focused)
                } else {
                    AccordionAction::None
                }
            }
            _ => AccordionAction::None,
        }
    }

    pub fn toggle(&mut self, index: usize) {
        if let Some(section) = self.sections.get_mut(index) {
            section.expanded = !section.expanded;
            self.focused = index;
        }
    }

    pub fn focus(&mut self, index: usize) {
        if index < self.sections.len() {
            self.focused = index;
            self.content_focus = false;
        }
    }

    pub fn enter_content(&mut self) {
        self.content_focus = true;
    }

    pub fn leave_content(&mut self) {
        self.content_focus = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_sections_match_typescript() {
        let accordion = AccordionState::new();
        assert_eq!(accordion.sections.len(), 4);
        assert_eq!(accordion.expanded_ids(), vec!["explorer", "source-control"]);
    }

    #[test]
    fn layout_splits_expanded_content() {
        let accordion = AccordionState::new();
        let layout = accordion.layout(Rect::new(0, 0, 36, 30));
        assert_eq!(layout.sections.len(), 4);
        assert_eq!(layout.sections[0].header.height, 1);
        assert!(layout.sections[0].content.height > 0);
        assert!(layout.sections[1].content.height > 0);
        assert_eq!(layout.sections[2].content.height, 0);
    }

    #[test]
    fn right_enters_expanded_content() {
        let mut accordion = AccordionState::new();
        let action = accordion.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(action, AccordionAction::EnterContent(0));
        assert!(accordion.content_focus);
        let action = accordion.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(action, AccordionAction::LeaveContent);
    }

    #[test]
    fn enter_toggles_section() {
        let mut accordion = AccordionState::new();
        accordion.focus(2);
        let action = accordion.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(action, AccordionAction::Toggle(2));
        assert!(accordion.sections[2].expanded);
    }
}
