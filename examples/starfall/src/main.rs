//! Starfall: catch the stars, dodge the rocks.
//!
//! Move with A/D or the arrow keys. Escape quits.
//!
//! The level lives in `assets/scenes/main.scene`. This file only holds the
//! behaviours that scene refers to.

use brandon::prelude::*;

/// Keeps score. Sits on the root node, where everything can find it.
#[behaviour]
#[derive(Default)]
struct Round {
    lives: u32,
    stars: u32,
    over: bool,
}

impl Round {
    /// The current round, from any behaviour.
    fn of<'a>(ctx: &'a mut Ctx) -> Option<&'a mut Round> {
        let id = ctx.scene.first_with::<Round>()?;
        ctx.scene.behaviour_mut::<Round>(id)
    }
}

impl Behaviour for Round {
    fn update(&mut self, _node: &mut Node, ctx: &mut Ctx) {
        if ctx.input.pressed(Key::Escape) {
            ctx.quit();
        }
        if self.over && ctx.input.pressed(Key::Enter) {
            ctx.change_scene("scenes/main.scene");
        }

        // Keep the on-screen display up to date.
        set_text(ctx, "Score", format!("Stars {}", self.stars));
        set_text(ctx, "Lives", format!("Lives {}", self.lives));
        if let Some(message) = ctx
            .scene
            .find("Message")
            .and_then(|id| ctx.scene.get_mut(id))
        {
            message.visible = self.over;
        }
    }
}

fn set_text(ctx: &mut Ctx, name: &str, text: String) {
    if let Some(label) = ctx
        .scene
        .find(name)
        .and_then(|id| ctx.scene.get_mut(id))
        .and_then(|n| n.text.as_mut())
    {
        label.text = text;
    }
}

/// The player's ship.
#[behaviour]
#[derive(Default)]
struct Ship {
    speed: f32,
    /// Seconds left of the red flash after hitting a rock.
    #[serde(skip)]
    hurt: f32,
}

impl Behaviour for Ship {
    fn update(&mut self, node: &mut Node, ctx: &mut Ctx) {
        let steer = (ctx.input.axis(Key::A, Key::D) + ctx.input.axis(Key::Left, Key::Right))
            .clamp(-1.0, 1.0);
        let x = node.transform.position.x + steer * self.speed * ctx.dt();
        node.transform.position.x = x.clamp(40.0, ctx.screen_size().x - 40.0);
        // Lean into the turn.
        node.transform.rotation +=
            (steer * 0.2 - node.transform.rotation) * (12.0 * ctx.dt()).min(1.0);

        for hit in ctx.scene.overlapping(node) {
            let Some(kind) = ctx.scene.behaviour::<Falling>(hit).map(|f| f.kind) else {
                continue;
            };
            ctx.destroy(hit);
            let Some(round) = Round::of(ctx) else {
                continue;
            };
            match kind {
                Kind::Star => round.stars += 1,
                Kind::Rock => {
                    round.lives = round.lives.saturating_sub(1);
                    round.over = round.lives == 0;
                    self.hurt = 0.4;
                }
            }
        }

        self.hurt = (self.hurt - ctx.dt()).max(0.0);
        if let Some(sprite) = &mut node.sprite {
            sprite.color = Color::WHITE.lerp(Color::hex(0xff4040), self.hurt / 0.4);
        }
        if Round::of(ctx).is_some_and(|round| round.over) {
            node.visible = false;
            node.shape = None;
        }
    }
}

/// Makes the engine flame flutter.
#[behaviour]
#[derive(Default)]
struct Flicker;

impl Behaviour for Flicker {
    fn update(&mut self, node: &mut Node, ctx: &mut Ctx) {
        let wave = (ctx.elapsed() * 38.0).sin() * 0.12;
        node.transform.scale = vec2(1.0 + wave * 0.5, 1.0 + wave + ctx.random.range(-0.08, 0.08));
    }
}

#[data]
#[derive(Copy, Default, PartialEq)]
enum Kind {
    #[default]
    Star,
    Rock,
}

/// Something falling from the sky.
#[behaviour]
#[derive(Default)]
struct Falling {
    speed: f32,
    spin: f32,
    kind: Kind,
}

impl Behaviour for Falling {
    fn update(&mut self, node: &mut Node, ctx: &mut Ctx) {
        node.transform.position.y += self.speed * ctx.dt();
        node.transform.rotation += self.spin * ctx.dt();
        if node.transform.position.y > ctx.screen_size().y + 60.0 {
            ctx.destroy(node.id());
        }
    }
}

/// Drops stars and rocks from the top of the screen, faster as time goes on.
#[behaviour]
#[derive(Default)]
struct Spawner {
    interval: f32,
    rock_chance: f32,
    #[serde(skip)]
    wait: f32,
    #[serde(skip)]
    age: f32,
}

impl Behaviour for Spawner {
    fn update(&mut self, _node: &mut Node, ctx: &mut Ctx) {
        if Round::of(ctx).is_some_and(|round| round.over) {
            return;
        }
        self.age += ctx.dt();
        self.wait -= ctx.dt();
        if self.wait > 0.0 {
            return;
        }
        // Twice as often after a minute.
        self.wait = self.interval / (1.0 + self.age / 60.0);
        let scene = if ctx.random.chance(self.rock_chance) {
            "scenes/rock.scene"
        } else {
            "scenes/star.scene"
        };
        let x = ctx.random.range(40.0, ctx.screen_size().x - 40.0);
        let size = ctx.random.range(0.8, 1.15);
        ctx.spawn(ctx.instantiate(scene).at(x, -50.0).scaled(size));
    }
}

/// Fills the background with slowly drifting dots.
#[behaviour]
#[derive(Default)]
struct Starfield {
    count: u32,
}

impl Behaviour for Starfield {
    fn ready(&mut self, node: &mut Node, ctx: &mut Ctx) {
        let screen = ctx.screen_size();
        for _ in 0..self.count {
            let depth = ctx.random.range(0.2, 1.0);
            let dot = Node::new("Dot")
                .at(
                    ctx.random.range(0.0, screen.x),
                    ctx.random.range(0.0, screen.y),
                )
                .with(
                    Sprite::new("dot.png")
                        .with_size(2.0 + depth * 5.0, 2.0 + depth * 5.0)
                        .with_color(Color::hex(0xbfd0ff).with_alpha(0.25 + depth * 0.6)),
                )
                .with(Drift {
                    speed: 12.0 + depth * 50.0,
                });
            ctx.spawn_child(node.id(), dot);
        }
    }
}

/// Moves a background dot down and wraps it back to the top.
struct Drift {
    speed: f32,
}

impl Behaviour for Drift {
    fn update(&mut self, node: &mut Node, ctx: &mut Ctx) {
        let screen = ctx.screen_size();
        node.transform.position.y += self.speed * ctx.dt();
        if node.transform.position.y > screen.y + 8.0 {
            node.transform.position = vec2(ctx.random.range(0.0, screen.x), -8.0);
        }
    }
}

fn main() {
    Game::new()
        .title("Starfall")
        .window_size(960, 540)
        .resizable(false)
        .background(Color::hex(0x0b1022))
        .register::<Round>()
        .register::<Ship>()
        .register::<Flicker>()
        .register::<Falling>()
        .register::<Spawner>()
        .register::<Starfield>()
        .start_scene("scenes/main.scene")
        .run();
}
