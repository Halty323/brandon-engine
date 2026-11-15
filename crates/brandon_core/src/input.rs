use glam::Vec2;
use std::collections::HashSet;

/// A key on the keyboard, named by its position on a US layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    A, B, C, D, E, F, G, H, I, J, K, L, M,
    N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9,
    Left, Right, Up, Down,
    Space, Enter, Escape, Tab, Backspace, Delete,
    LeftShift, RightShift, LeftCtrl, RightCtrl, LeftAlt, RightAlt,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
}

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// The state of the keyboard and mouse for the current frame.
#[derive(Debug, Default)]
pub struct Input {
    keys_down: HashSet<Key>,
    keys_pressed: HashSet<Key>,
    keys_released: HashSet<Key>,
    buttons_down: HashSet<MouseButton>,
    buttons_pressed: HashSet<MouseButton>,
    buttons_released: HashSet<MouseButton>,
    mouse_position: Vec2,
    mouse_delta: Vec2,
    scroll: Vec2,
}

impl Input {
    /// Is the key held down?
    pub fn down(&self, key: Key) -> bool {
        self.keys_down.contains(&key)
    }

    /// Did the key go down this frame?
    pub fn pressed(&self, key: Key) -> bool {
        self.keys_pressed.contains(&key)
    }

    /// Did the key go up this frame?
    pub fn released(&self, key: Key) -> bool {
        self.keys_released.contains(&key)
    }

    /// `-1.0` while `negative` is held, `1.0` while `positive` is held, `0.0` otherwise.
    pub fn axis(&self, negative: Key, positive: Key) -> f32 {
        (self.down(positive) as i8 - self.down(negative) as i8) as f32
    }

    /// A direction from four keys, never longer than `1.0`, so diagonals are
    /// not faster. Up is negative `y`, matching screen coordinates.
    pub fn axis2d(&self, left: Key, right: Key, up: Key, down: Key) -> Vec2 {
        Vec2::new(self.axis(left, right), self.axis(up, down)).normalize_or_zero()
    }

    /// Is the mouse button held down?
    pub fn mouse_down(&self, button: MouseButton) -> bool {
        self.buttons_down.contains(&button)
    }

    /// Did the mouse button go down this frame?
    pub fn mouse_pressed(&self, button: MouseButton) -> bool {
        self.buttons_pressed.contains(&button)
    }

    /// Did the mouse button go up this frame?
    pub fn mouse_released(&self, button: MouseButton) -> bool {
        self.buttons_released.contains(&button)
    }

    /// Mouse position in the window, in pixels from the top-left corner.
    pub fn mouse_position(&self) -> Vec2 {
        self.mouse_position
    }

    /// How far the mouse moved this frame.
    pub fn mouse_delta(&self) -> Vec2 {
        self.mouse_delta
    }

    /// Scroll wheel movement this frame, in lines.
    pub fn scroll(&self) -> Vec2 {
        self.scroll
    }

    // --- Fed by the platform layer -------------------------------------

    /// Records a key going down or up.
    pub fn set_key(&mut self, key: Key, down: bool) {
        if down {
            if self.keys_down.insert(key) {
                self.keys_pressed.insert(key);
            }
        } else if self.keys_down.remove(&key) {
            self.keys_released.insert(key);
        }
    }

    /// Records a mouse button going down or up.
    pub fn set_mouse_button(&mut self, button: MouseButton, down: bool) {
        if down {
            if self.buttons_down.insert(button) {
                self.buttons_pressed.insert(button);
            }
        } else if self.buttons_down.remove(&button) {
            self.buttons_released.insert(button);
        }
    }

    /// Records the mouse position.
    pub fn set_mouse_position(&mut self, position: Vec2) {
        self.mouse_delta += position - self.mouse_position;
        self.mouse_position = position;
    }

    /// Records scroll wheel movement.
    pub fn add_scroll(&mut self, delta: Vec2) {
        self.scroll += delta;
    }

    /// Releases everything, for example when the window loses focus.
    pub fn release_all(&mut self) {
        for key in self.keys_down.drain() {
            self.keys_released.insert(key);
        }
        for button in self.buttons_down.drain() {
            self.buttons_released.insert(button);
        }
    }

    /// Clears the per-frame state. Call once at the end of every frame.
    pub fn end_frame(&mut self) {
        self.keys_pressed.clear();
        self.keys_released.clear();
        self.buttons_pressed.clear();
        self.buttons_released.clear();
        self.mouse_delta = Vec2::ZERO;
        self.scroll = Vec2::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_hold_release() {
        let mut input = Input::default();
        input.set_key(Key::A, true);
        assert!(input.down(Key::A) && input.pressed(Key::A));
        input.end_frame();
        input.set_key(Key::A, true); // key repeat
        assert!(input.down(Key::A) && !input.pressed(Key::A));
        input.set_key(Key::A, false);
        assert!(!input.down(Key::A) && input.released(Key::A));
    }

    #[test]
    fn diagonal_is_not_faster() {
        let mut input = Input::default();
        input.set_key(Key::D, true);
        input.set_key(Key::S, true);
        let dir = input.axis2d(Key::A, Key::D, Key::W, Key::S);
        assert!((dir.length() - 1.0).abs() < 1e-6);
        assert!(dir.x > 0.0 && dir.y > 0.0);
    }

    #[test]
    fn losing_focus_releases_keys() {
        let mut input = Input::default();
        input.set_key(Key::Space, true);
        input.end_frame();
        input.release_all();
        assert!(!input.down(Key::Space) && input.released(Key::Space));
    }
}
