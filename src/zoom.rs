//! Session-local whole-UI zoom and its keyboard shortcuts.

use iced::keyboard::{Key, Modifiers};

#[derive(Debug, Clone, Copy)]
pub enum Action {
    In,
    Out,
    Reset,
}

pub struct Zoom(u16);

impl Default for Zoom {
    fn default() -> Self {
        Self(100)
    }
}

impl Zoom {
    pub fn scale_factor(&self) -> f32 {
        f32::from(self.0) / 100.0
    }

    pub fn apply(&mut self, action: Action) {
        self.0 = match action {
            Action::In => (self.0 + 10).min(200),
            Action::Out => self.0.saturating_sub(10).max(50),
            Action::Reset => 100,
        };
    }
}

pub fn shortcut(key: &Key, modifiers: Modifiers) -> Option<Action> {
    if !modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }
    match key.as_ref() {
        Key::Character("+" | "=") => Some(Action::In),
        Key::Character("-") => Some(Action::Out),
        Key::Character("0") => Some(Action::Reset),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_steps_bounds_and_reset() {
        let mut zoom = Zoom::default();
        assert_eq!(zoom.scale_factor(), 1.0);
        zoom.apply(Action::In);
        assert_eq!(zoom.scale_factor(), 1.1);
        zoom.apply(Action::Out);
        assert_eq!(zoom.scale_factor(), 1.0);
        for _ in 0..100 {
            zoom.apply(Action::In);
        }
        assert_eq!(zoom.scale_factor(), 2.0);
        zoom.apply(Action::Reset);
        assert_eq!(zoom.scale_factor(), 1.0);
        for _ in 0..100 {
            zoom.apply(Action::Out);
        }
        assert_eq!(zoom.scale_factor(), 0.5);
        zoom.apply(Action::Reset);
        assert_eq!(zoom.scale_factor(), 1.0);
    }

    #[test]
    fn control_shortcuts_allow_shift_but_not_alt_or_logo() {
        for modifiers in [Modifiers::CTRL, Modifiers::CTRL | Modifiers::SHIFT] {
            for key in ["+", "="] {
                assert!(matches!(
                    shortcut(&Key::Character(key.into()), modifiers),
                    Some(Action::In)
                ));
            }
            assert!(matches!(
                shortcut(&Key::Character("-".into()), modifiers),
                Some(Action::Out)
            ));
            assert!(matches!(
                shortcut(&Key::Character("0".into()), modifiers),
                Some(Action::Reset)
            ));
        }
        for modifiers in [
            Modifiers::empty(),
            Modifiers::SHIFT,
            Modifiers::CTRL | Modifiers::ALT,
            Modifiers::CTRL | Modifiers::LOGO,
        ] {
            assert!(shortcut(&Key::Character("=".into()), modifiers).is_none());
        }
        assert!(shortcut(&Key::Character("a".into()), Modifiers::CTRL).is_none());
    }
}
