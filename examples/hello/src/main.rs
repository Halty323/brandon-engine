//! The smallest Brandon game: a scene built in code, with no asset files.
//!
//! Move with WASD or the arrow keys. Zoom with Q and E. Escape quits.

use brandon::prelude::*;

/// Moves its node with the keyboard.
struct Player {
    speed: f32,
}

impl Behaviour for Player {
    fn update(&mut self, node: &mut Node, ctx: &mut Ctx) {
        let direction = ctx.input.axis2d(Key::A, Key::D, Key::W, Key::S)
            + ctx.input.axis2d(Key::Left, Key::Right, Key::Up, Key::Down);
        node.transform.position += direction.clamp_length_max(1.0) * self.speed * ctx.dt();
        if ctx.input.pressed(Key::Escape) {
            ctx.quit();
        }
    }
}

/// Turns its node around, `speed` radians per second.
struct Spin {
    speed: f32,
}

impl Behaviour for Spin {
    fn update(&mut self, node: &mut Node, ctx: &mut Ctx) {
        node.transform.rotation += self.speed * ctx.dt();
    }
}

/// Zooms the camera on its node with Q and E.
struct Zoom;

impl Behaviour for Zoom {
    fn update(&mut self, node: &mut Node, ctx: &mut Ctx) {
        if let Some(camera) = &mut node.camera {
            let change = ctx.input.axis(Key::Q, Key::E) * ctx.dt();
            camera.zoom = (camera.zoom * (1.0 + change)).clamp(0.25, 4.0);
        }
    }
}

fn main() {
    Game::new()
        .title("Hello, Brandon")
        .setup(|scene| {
            // A floor of tiles, so movement is visible.
            for row in -6..=6 {
                for column in -10..=10 {
                    let shade = if (row + column) % 2 == 0 {
                        0x1c2233
                    } else {
                        0x232b40
                    };
                    scene.spawn(
                        Node::new("Tile")
                            .at(column as f32 * 96.0, row as f32 * 96.0)
                            .z(-1)
                            .with(Sprite::rect(96.0, 96.0, Color::hex(shade))),
                    );
                }
            }

            // Children move, turn and scale with their parent.
            scene.spawn(
                Node::new("Windmill")
                    .at(260.0, -40.0)
                    .with(Sprite::rect(24.0, 24.0, Color::hex(0x7bdff2)))
                    .with(Spin { speed: 1.2 })
                    .child(Node::new("Blade").at(70.0, 0.0).with(Sprite::rect(
                        100.0,
                        14.0,
                        Color::hex(0xb2f7ef),
                    )))
                    .child(Node::new("Blade").at(-70.0, 0.0).with(Sprite::rect(
                        100.0,
                        14.0,
                        Color::hex(0xb2f7ef),
                    )))
                    .child(
                        Node::new("Label").at(0.0, 30.0).with(
                            Text::new("children follow")
                                .with_size(16.0)
                                .with_align(Align::Center),
                        ),
                    ),
            );

            // The camera is a child of the player, so the view follows it.
            scene.spawn(
                Node::new("Player")
                    .z(1)
                    .with(Sprite::rect(48.0, 48.0, Color::hex(0xffb000)))
                    .with(Player { speed: 320.0 })
                    .child(Node::new("Camera").with(Camera::new()).with(Zoom))
                    .child(
                        Node::new("Name")
                            .at(0.0, -62.0)
                            .with(Text::new("you").with_size(20.0).with_align(Align::Center)),
                    ),
            );

            // An overlay stays fixed to the window, whatever the camera does.
            scene.spawn(
                Node::new("Help").at(16.0, 12.0).with(Overlay).with(
                    Text::new("Move: WASD or arrows    Zoom: Q / E")
                        .with_size(20.0)
                        .with_color(Color::hex(0x9aa4c0)),
                ),
            );
        })
        .run();
}
