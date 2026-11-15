use brandon_core::{Assets, Color, Input, Random, Time};
use brandon_scene::*;
use glam::Vec2;
use serde::{Deserialize, Serialize};

fn run_frame(scene: &mut Scene, assets: &Assets, library: &SceneLibrary) -> Requests {
    let mut time = Time::default();
    time.advance(0.5);
    let mut requests = Requests::default();
    scene.update(
        &Input::default(),
        &time,
        &mut Random::from_seed(1),
        assets,
        library,
        &View::default(),
        &mut requests,
    );
    requests
}

fn run(scene: &mut Scene) -> Requests {
    run_frame(scene, &Assets::at("/nonexistent"), &SceneLibrary::new())
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Mover {
    speed: f32,
}

impl Behaviour for Mover {
    fn update(&mut self, node: &mut Node, ctx: &mut Ctx) {
        node.transform.position.x += self.speed * ctx.dt();
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Counter {
    ready_calls: u32,
    updates: u32,
}

impl Behaviour for Counter {
    fn ready(&mut self, _node: &mut Node, _ctx: &mut Ctx) {
        self.ready_calls += 1;
    }
    fn update(&mut self, _node: &mut Node, _ctx: &mut Ctx) {
        self.updates += 1;
    }
}

#[test]
fn tree_structure_and_order() {
    let mut scene = Scene::new();
    let a = scene.spawn(
        Node::new("a")
            .child(Node::new("a1").child(Node::new("a1x")))
            .child(Node::new("a2")),
    );
    let b = scene.spawn(Node::new("b"));
    let names: Vec<_> = scene
        .ids()
        .iter()
        .map(|id| scene.get(*id).unwrap().name.clone())
        .collect();
    assert_eq!(names, ["a", "a1", "a1x", "a2", "b"]);
    assert_eq!(scene.len(), 5);
    assert_eq!(scene.roots(), [a, b]);

    let a1 = scene.find("a1").unwrap();
    assert_eq!(scene.parent(a1), Some(a));
    assert_eq!(scene.find_under(a, "a1x"), scene.find("a1x"));
    assert_eq!(scene.find_under(b, "a1x"), None);

    // Removing a node removes everything under it, and old ids stay dead.
    let a1x = scene.find("a1x").unwrap();
    assert!(scene.remove(a1));
    assert!(!scene.contains(a1) && !scene.contains(a1x));
    assert_eq!(scene.len(), 3);
    let reused = scene.spawn(Node::new("new"));
    assert!(!scene.contains(a1) && !scene.contains(a1x));
    assert!(scene.get(a1x).is_none());
    assert_eq!(scene.get(reused).unwrap().name, "new");
    assert!(!scene.remove(a1));
}

#[test]
fn set_parent_refuses_cycles() {
    let mut scene = Scene::new();
    let a = scene.spawn(Node::new("a"));
    let b = scene.spawn_child(a, Node::new("b"));
    let c = scene.spawn_child(b, Node::new("c"));
    assert!(!scene.set_parent(a, Some(c)));
    assert!(!scene.set_parent(a, Some(a)));
    assert!(scene.set_parent(c, None));
    assert_eq!(scene.roots(), [a, c]);
    assert!(scene.set_parent(c, Some(a)));
    assert_eq!(scene.children(a), [b, c]);
    assert!(scene.roots() == [a]);

    assert!(scene.move_to(c, Some(a), 0));
    assert_eq!(scene.children(a), [c, b]);
    assert!(scene.move_to(c, Some(a), 99));
    assert_eq!(scene.children(a), [b, c]);
    assert!(scene.move_to(b, None, 0));
    assert_eq!(scene.roots(), [b, a]);
    assert!(
        !scene.move_to(a, Some(c), 0),
        "a node cannot go inside its own child"
    );
    assert_eq!(scene.roots(), [b, a]);
}

#[test]
fn spawn_child_of_dead_parent_is_dropped() {
    let mut scene = Scene::new();
    let a = scene.spawn(Node::new("a"));
    scene.remove(a);
    assert!(scene.spawn_child(a, Node::new("orphan")).is_none());
    assert!(scene.is_empty());
}

#[test]
fn world_transforms_follow_parents() {
    let mut scene = Scene::new();
    let parent = scene.spawn(Node::new("p").at(100.0, 50.0).scaled(2.0).z(3));
    let child = scene.spawn_child(parent, Node::new("c").at(10.0, 0.0).z(1));
    assert_eq!(scene.world_position(child), Vec2::new(120.0, 50.0));

    scene.get_mut(parent).unwrap().transform.rotation = std::f32::consts::FRAC_PI_2;
    assert_eq!(
        scene.world_position(child),
        Vec2::new(120.0, 50.0),
        "cached until refreshed"
    );
    scene.refresh_transforms();
    assert!(scene
        .world_position(child)
        .abs_diff_eq(Vec2::new(100.0, 70.0), 1e-4));

    let mut seen = Vec::new();
    scene.for_each_visible(|node, v| seen.push((node.name.clone(), v.z, v.overlay)));
    assert_eq!(
        seen,
        [("p".to_string(), 3, false), ("c".to_string(), 4, false)]
    );
}

#[test]
fn hidden_and_overlay_are_inherited() {
    let mut scene = Scene::new();
    scene.spawn(Node::new("hud").with(Overlay).child(Node::new("score")));
    scene.spawn(
        Node::new("hidden")
            .hidden()
            .child(Node::new("hidden child")),
    );
    scene.spawn(Node::new("world"));
    let mut seen = Vec::new();
    scene.for_each_visible(|node, v| seen.push((node.name.clone(), v.overlay)));
    assert_eq!(
        seen,
        [
            ("hud".to_string(), true),
            ("score".to_string(), true),
            ("world".to_string(), false)
        ]
    );
}

#[test]
fn behaviours_update_and_get_ready_once() {
    let mut scene = Scene::new();
    let id = scene.spawn(
        Node::new("n")
            .with(Mover { speed: 10.0 })
            .with(Counter::default()),
    );
    run(&mut scene);
    run(&mut scene);
    let node = scene.get(id).unwrap();
    assert_eq!(
        node.transform.position.x,
        2.0 * 10.0 * 0.1,
        "delta is clamped to Time::MAX_DELTA"
    );
    let counter = scene.behaviour::<Counter>(id).unwrap();
    assert_eq!((counter.ready_calls, counter.updates), (1, 2));
    assert!(node.has::<Mover>() && node.behaviour_count() == 2);
}

struct DestroySelf;
impl Behaviour for DestroySelf {
    fn update(&mut self, node: &mut Node, ctx: &mut Ctx) {
        assert!(
            ctx.scene.get(node.id()).is_none(),
            "the running node is lent out"
        );
        assert!(ctx.scene.contains(node.id()));
        ctx.destroy(node.id());
    }
}

struct MustNotRun;
impl Behaviour for MustNotRun {
    fn update(&mut self, _node: &mut Node, _ctx: &mut Ctx) {
        panic!("ran on a destroyed node");
    }
}

#[test]
fn a_node_can_destroy_itself() {
    let mut scene = Scene::new();
    let id = scene.spawn(
        Node::new("n")
            .with(DestroySelf)
            .with(MustNotRun)
            .child(Node::new("child").with(MustNotRun)),
    );
    let other = scene.spawn(Node::new("other").with(Counter::default()));
    run(&mut scene);
    assert!(!scene.contains(id));
    assert_eq!(scene.len(), 1);
    assert_eq!(scene.behaviour::<Counter>(other).unwrap().updates, 1);
}

struct Spawner;
impl Behaviour for Spawner {
    fn update(&mut self, node: &mut Node, ctx: &mut Ctx) {
        ctx.spawn_child(node.id(), Node::new("bullet").with(Counter::default()));
        ctx.spawn(Node::new("root bullet"));
    }
}

#[test]
fn spawning_during_update() {
    let mut scene = Scene::new();
    let id = scene.spawn(Node::new("gun").with(Spawner));
    run(&mut scene);
    assert_eq!(scene.children(id).len(), 1);
    let bullet = scene.children(id)[0];
    assert_eq!(
        scene.behaviour::<Counter>(bullet).unwrap().updates,
        0,
        "new nodes start next frame"
    );
    run(&mut scene);
    assert_eq!(scene.children(id).len(), 2);
    assert_eq!(scene.behaviour::<Counter>(bullet).unwrap().updates, 1);
    assert_eq!(scene.roots().len(), 3);
}

struct RemoveOther;
impl Behaviour for RemoveOther {
    fn update(&mut self, node: &mut Node, _ctx: &mut Ctx) {
        assert!(
            !node.has::<RemoveOther>(),
            "a running behaviour does not see itself"
        );
        assert!(node.remove_behaviour::<Counter>());
        assert!(!node.remove_behaviour::<Counter>());
    }
}

#[test]
fn removing_a_sibling_behaviour_while_running() {
    let mut scene = Scene::new();
    let id = scene.spawn(
        Node::new("n")
            .with(RemoveOther)
            .with(Counter::default())
            .with(Mover { speed: 1.0 }),
    );
    run(&mut scene);
    let node = scene.get(id).unwrap();
    assert!(!node.has::<Counter>() && node.has::<RemoveOther>() && node.has::<Mover>());
    assert_eq!(node.behaviour_count(), 2);
    assert!(node.transform.position.x > 0.0);
}

struct Asker;
impl Behaviour for Asker {
    fn update(&mut self, _node: &mut Node, ctx: &mut Ctx) {
        ctx.change_scene("next.scene");
        ctx.quit();
    }
}

#[test]
fn requests_reach_the_engine() {
    let mut scene = Scene::new();
    scene.spawn(Node::new("n").with(Asker));
    let requests = run(&mut scene);
    assert!(requests.quit);
    assert_eq!(requests.change_scene.as_deref(), Some("next.scene"));
}

#[test]
fn view_transform_with_and_without_camera() {
    let mut scene = Scene::new();
    let window = Vec2::new(800.0, 600.0);
    assert_eq!(
        scene
            .view_transform(window)
            .transform_point2(Vec2::new(5.0, 7.0)),
        Vec2::new(5.0, 7.0)
    );

    let cam = scene.spawn(
        Node::new("cam")
            .at(1000.0, 2000.0)
            .with(Camera::new().with_zoom(2.0)),
    );
    let view = scene.view_transform(window);
    assert!(view
        .transform_point2(Vec2::new(1000.0, 2000.0))
        .abs_diff_eq(Vec2::new(400.0, 300.0), 1e-3));
    assert!(view
        .transform_point2(Vec2::new(1010.0, 2000.0))
        .abs_diff_eq(Vec2::new(420.0, 300.0), 1e-3));

    scene.get_mut(cam).unwrap().camera.as_mut().unwrap().active = false;
    assert_eq!(scene.view_transform(window), glam::Affine2::IDENTITY);
}

// --- Scene files ---------------------------------------------------------

fn temp_assets(name: &str, files: &[(&str, &str)]) -> Assets {
    let dir =
        std::env::temp_dir().join(format!("brandon-scene-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (file, text) in files {
        std::fs::write(dir.join(file), text).unwrap();
    }
    Assets::at(dir)
}

#[test]
fn scene_text_round_trip() {
    register::<Mover>();
    register::<Counter>();
    let mut scene = Scene::new();
    let root = scene.spawn(
        Node::new("Root")
            .child(
                Node::new("Player")
                    .at(100.0, 200.0)
                    .z(2)
                    .with(
                        Sprite::new("player.png")
                            .with_color(Color::hex(0xff8800))
                            .pixelated(),
                    )
                    .with(Shape::circle(16.0))
                    .with(Mover { speed: 42.5 })
                    .with(DestroySelf) // not registered: left out with a warning
                    .child(Node::new("Camera").with(Camera::new().with_zoom(2.0))),
            )
            .child(
                Node::new("Hud").with(Overlay).with(
                    Text::new("Score 0")
                        .with_size(32.0)
                        .with_align(Align::Center),
                ),
            )
            .child(
                Node::new("Wall")
                    .hidden()
                    .with(Sprite::rect(10.0, 300.0, Color::GRAY))
                    .with(Shape::rect(10.0, 300.0)),
            ),
    );
    let text = scene.to_ron(root).unwrap();
    println!("{text}");
    assert!(
        text.contains("\"Mover\": (") && text.contains("speed: 42.5"),
        "{text}"
    );
    assert!(text.contains("\"#ff8800\""), "{text}");
    assert!(
        !text.contains("Some("),
        "options are written without Some(): {text}"
    );
    assert!(!text.contains("flip_x"), "defaults are left out: {text}");

    let mut loaded = Scene::new();
    let loaded_root = loaded.spawn(Node::from_ron(&text).unwrap());
    assert_eq!(loaded.len(), scene.len());
    let player = loaded.get(loaded.find("Player").unwrap()).unwrap();
    assert_eq!(
        player.transform,
        scene.get(scene.find("Player").unwrap()).unwrap().transform
    );
    assert_eq!(
        player.sprite,
        scene.get(scene.find("Player").unwrap()).unwrap().sprite
    );
    assert_eq!(player.shape, Some(Shape::circle(16.0)));
    assert_eq!(player.behaviour::<Mover>().unwrap().speed, 42.5);
    assert_eq!(player.behaviour_count(), 1);
    let hud = loaded.get(loaded.find("Hud").unwrap()).unwrap();
    assert_eq!(hud.overlay, Some(Overlay));
    assert_eq!(hud.text.as_ref().unwrap().align, Align::Center);
    assert!(!loaded.get(loaded.find("Wall").unwrap()).unwrap().visible);
    assert_eq!(
        loaded.to_ron(loaded_root).unwrap(),
        text,
        "saving a loaded scene gives the same text"
    );
}

#[test]
fn hand_written_scene_is_forgiving() {
    register::<Mover>();
    let node = Node::from_ron(
        r##"
        // Whole numbers are fine where decimals are expected.
        (
            name: "Ship",
            transform: (position: (480, 470), z: 1),
            sprite: (texture: "ship.png", color: "#fff"),
            shape: Circle(radius: 20),
            behaviours: { "Mover": (speed: 3) },
            children: [ (name: "Flame", transform: (position: (0, 30))) ],
        )
        "##,
    )
    .unwrap();
    assert_eq!(node.transform.position, Vec2::new(480.0, 470.0));
    assert_eq!(node.behaviour::<Mover>().unwrap().speed, 3.0);
    let mut scene = Scene::new();
    let id = scene.spawn(node);
    assert_eq!(
        scene.world_position(scene.children(id)[0]),
        Vec2::new(480.0, 500.0)
    );
}

#[test]
fn scene_mistakes_are_explained() {
    register::<Mover>();
    let unknown = Node::from_ron(r#"(name: "x", behaviours: { "Movr": () })"#)
        .unwrap_err()
        .to_string();
    assert!(
        unknown.contains("unknown behaviour `Movr`") && unknown.contains("Mover"),
        "{unknown}"
    );
    assert!(unknown.contains("line 1"), "{unknown}");

    let typo = Node::from_ron("(\n  name: \"x\",\n  sprit: (),\n)")
        .unwrap_err()
        .to_string();
    assert!(typo.contains("line 3") && typo.contains("sprit"), "{typo}");

    let bad_field = Node::from_ron(r#"(behaviours: { "Mover": (speeed: 1) })"#);
    assert!(
        bad_field.is_ok(),
        "Mover uses serde(default) without deny_unknown_fields"
    );
}

#[test]
fn scenes_inside_scenes() {
    register::<Mover>();
    let assets = temp_assets(
        "instances",
        &[
            (
                "rock.scene",
                r#"(name: "Rock", sprite: (texture: "rock.png"), shape: Circle(radius: 8),
                    behaviours: { "Mover": (speed: 5) }, children: [(name: "Dust")])"#,
            ),
            (
                "main.scene",
                r##"(name: "Main", children: [
                    (instance: "rock.scene", transform: (position: (10, 20))),
                    (instance: "rock.scene", name: "Big rock", transform: (scale: (3, 3)),
                     sprite: (texture: "rock.png", color: "#f00"), children: [(name: "Extra")]),
                ])"##,
            ),
            (
                "loop_a.scene",
                r#"(name: "A", children: [(instance: "loop_b.scene")])"#,
            ),
            (
                "loop_b.scene",
                r#"(name: "B", children: [(instance: "loop_a.scene")])"#,
            ),
        ],
    );
    let library = SceneLibrary::new();
    let mut scene = Scene::new();
    let root = scene.spawn(library.load(&assets, "main.scene").unwrap());
    assert_eq!(scene.len(), 6);

    let rock = scene.get(scene.children(root)[0]).unwrap();
    assert_eq!(
        (rock.name.as_str(), rock.instance_of()),
        ("Rock", Some("rock.scene"))
    );
    assert_eq!(rock.transform.position, Vec2::new(10.0, 20.0));
    assert_eq!(rock.behaviour::<Mover>().unwrap().speed, 5.0);

    let big = scene.get(scene.children(root)[1]).unwrap();
    assert_eq!(big.name, "Big rock");
    assert_eq!(big.sprite.as_ref().unwrap().color, Color::RED);
    assert_eq!(big.shape, Some(Shape::circle(8.0)));
    let big_children: Vec<_> = scene
        .children(big.id())
        .iter()
        .map(|c| scene.get(*c).unwrap().name.clone())
        .collect();
    assert_eq!(big_children, ["Dust", "Extra"]);

    // Each copy has its own behaviour state.
    run_frame(&mut scene, &assets, &library);
    assert!(
        scene
            .get(scene.children(root)[0])
            .unwrap()
            .transform
            .position
            .x
            > 10.0
    );
    let second = library.instantiate(&assets, "rock.scene").unwrap();
    assert_eq!(second.transform.position, Vec2::ZERO);

    // Placed scenes are saved as references.
    let saved = scene.to_ron(root).unwrap();
    assert_eq!(
        saved.matches("instance: \"rock.scene\"").count(),
        2,
        "{saved}"
    );
    assert!(
        !saved.contains("Dust") && !saved.contains("Mover"),
        "{saved}"
    );

    let cycle = library
        .load(&assets, "loop_a.scene")
        .unwrap_err()
        .to_string();
    assert!(
        cycle.contains("loop_a.scene -> loop_b.scene -> loop_a.scene"),
        "{cycle}"
    );

    let missing = library.load(&assets, "nope.scene").unwrap_err().to_string();
    assert!(
        missing.contains("nope.scene") && missing.contains("not found"),
        "{missing}"
    );
    let placeholder = library.instantiate_or_placeholder(&assets, "nope.scene");
    assert_eq!(placeholder.name, "<missing nope.scene>");
}

struct SpawnRock;
impl Behaviour for SpawnRock {
    fn update(&mut self, _node: &mut Node, ctx: &mut Ctx) {
        ctx.spawn(ctx.instantiate("rock.scene").at(7.0, 9.0));
    }
}

#[test]
fn instantiate_from_a_behaviour() {
    let assets = temp_assets(
        "spawn",
        &[("rock.scene", r#"(name: "Rock", shape: Circle(radius: 8))"#)],
    );
    let library = SceneLibrary::new();
    let mut scene = Scene::new();
    scene.spawn(Node::new("spawner").with(SpawnRock));
    run_frame(&mut scene, &assets, &library);
    std::fs::remove_file(assets.roots()[0].join("rock.scene")).unwrap();
    run_frame(&mut scene, &assets, &library); // served from the library's cache
    let rocks: Vec<_> = scene
        .ids()
        .into_iter()
        .filter(|id| scene.get(*id).unwrap().name == "Rock")
        .collect();
    assert_eq!(rocks.len(), 2);
    assert_eq!(
        scene.get(rocks[0]).unwrap().transform.position,
        Vec2::new(7.0, 9.0)
    );
}

#[test]
fn unknown_behaviours_can_be_kept_for_tools() {
    // This test flips a global switch, so it uses a behaviour name no other test uses.
    let text = r#"(name: "x", behaviours: {
        "OnlyTheGameKnowsThis": (speed: 460, kind: Rock, path: [(1, 2), (3, 4)], label: "a) b"),
        "AlsoUnknownUnit": (),
    })"#;
    assert!(Node::from_ron(text).is_err());

    keep_unknown_behaviours(true);
    let loaded = Node::from_ron(text);
    keep_unknown_behaviours(false);
    let node = loaded.unwrap();

    let raws: Vec<&RawBehaviour> = node.behaviours_of::<RawBehaviour>().collect();
    assert_eq!(raws.len(), 2);
    assert_eq!(raws[0].name, "OnlyTheGameKnowsThis");
    assert_eq!(
        raws[0].ron,
        r#"(speed: 460, kind: Rock, path: [(1, 2), (3, 4)], label: "a) b")"#
    );
    assert_eq!(raws[1], &RawBehaviour::new("AlsoUnknownUnit", "()"));

    // Saved again, the data is unchanged, and edits made by a tool are kept.
    let mut scene = Scene::new();
    let id = scene.spawn(node);
    let saved = scene.to_ron(id).unwrap();
    // Too long for one line, so it is laid out over several.
    let expected = r#"        "OnlyTheGameKnowsThis": (
            speed: 460,
            kind: Rock,
            path: [(1, 2), (3, 4)],
            label: "a) b",
        ),"#;
    assert!(saved.contains(expected), "{saved}");

    let node = scene.get_mut(id).unwrap();
    node.behaviours_of_mut::<RawBehaviour>().next().unwrap().ron = "(speed: 500)".to_string();
    assert!(node.remove_nth_behaviour::<RawBehaviour>(1));
    assert!(!node.remove_nth_behaviour::<RawBehaviour>(1));
    let saved = scene.to_ron(id).unwrap();
    assert!(
        saved.contains(r#""OnlyTheGameKnowsThis": (speed: 500)"#)
            && !saved.contains("AlsoUnknownUnit"),
        "{saved}"
    );

    // Broken data is refused when saving instead of writing a broken file.
    scene
        .get_mut(id)
        .unwrap()
        .behaviours_of_mut::<RawBehaviour>()
        .next()
        .unwrap()
        .ron = "(speed: ".to_string();
    let err = scene.to_ron(id).unwrap_err().to_string();
    assert!(err.contains("OnlyTheGameKnowsThis"), "{err}");
}
