//! `snapshot` and `query`: the window's tree as YAML in Playwright's aria-snapshot shape (roles,
//! names, states, values, refs, bounds on request), in the inspection and screen-reader views,
//! rooted at a ref, and the nodes a query matches.
//!
//! `cargo test -p gpux-devtools --test snapshot`

mod support;

use gpux::TestAppContext;
use serde_json::json;
use support::*;

fn yaml(cx: &mut TestAppContext, params: serde_json::Value) -> String {
    request(cx, "snapshot", params).unwrap()["snapshot"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn the_snapshot_has_playwright_s_aria_shape() {
    let mut cx = TestAppContext::single();
    let (_window, _guard) = controls(&mut cx);
    let yaml = yaml(&mut cx, json!({}));
    let lines: Vec<&str> = yaml.lines().collect();
    assert!(lines[0].starts_with("- window"), "{yaml}");
    let line = |prefix: &str| {
        lines
            .iter()
            .find(|line| line.trim_start().starts_with(prefix))
            .unwrap_or_else(|| panic!("no `{prefix}` line in\n{yaml}"))
            .trim_start()
            .to_string()
    };
    assert!(line("- button \"Save\" [ref=").ends_with(']'), "{yaml}");
    assert!(line("- checkbox \"Wrap\" [checked=false] [ref=").ends_with(']'));
    assert!(line("- slider \"Volume\"").ends_with(": \"5\""), "{yaml}");
    assert!(line("- button \"Details\" [expanded=false]").contains("[ref="));
    assert!(line("- text [ref=").contains(": status"), "{yaml}");
    // Children nest under their parent, two spaces deeper.
    let depth = |text: &str| {
        lines.iter().find(|l| l.contains(text)).unwrap().len()
            - lines
                .iter()
                .find(|l| l.contains(text))
                .unwrap()
                .trim_start()
                .len()
    };
    assert!(depth("\"Save\"") > depth("- window"));
}

#[test]
fn refs_name_nodes_that_queries_and_actions_reach() {
    let mut cx = TestAppContext::single();
    let (_window, _guard) = controls(&mut cx);
    let yaml = yaml(&mut cx, json!({}));
    let save = reference(&mut cx, "role=button name=Save");
    assert!(yaml.contains(&format!("\"Save\" [ref={save}]")), "{yaml}");
}

#[test]
fn bounds_come_on_request() {
    let mut cx = TestAppContext::single();
    let (_window, _guard) = controls(&mut cx);
    assert!(!yaml(&mut cx, json!({})).contains("[bounds="));
    let with = yaml(&mut cx, json!({"bounds": true}));
    let save = with.lines().find(|l| l.contains("\"Save\"")).unwrap();
    assert!(save.contains("[bounds=0,0,80,20]"), "{save}");
}

#[test]
fn the_screen_reader_view_leaves_out_generic_containers() {
    let mut cx = TestAppContext::single();
    let (_window, _guard) = controls(&mut cx);
    let inspection = yaml(&mut cx, json!({}));
    let screen_reader = yaml(&mut cx, json!({"view": "screen-reader"}));
    assert!(inspection.contains("- generic"), "{inspection}");
    assert!(!screen_reader.contains("- generic"), "{screen_reader}");
    assert!(screen_reader.contains("- button \"Save\""));
}

#[test]
fn a_snapshot_can_start_at_a_ref() {
    let mut cx = TestAppContext::single();
    let (_window, _guard) = controls(&mut cx);
    let shelf = reference(&mut cx, "id=shelf");
    let yaml = yaml(&mut cx, json!({"ref": shelf}));
    assert!(yaml.starts_with("- generic [ref="), "{yaml}");
    assert!(yaml.contains("\"Far\""));
    assert!(!yaml.contains("\"Save\""), "{yaml}");
    let gone = request(&mut cx, "snapshot", json!({"ref": "424242"})).unwrap_err();
    assert!(gone.message.contains("424242"), "{gone}");
}

#[test]
fn query_returns_every_match_with_its_states_and_bounds() {
    let mut cx = TestAppContext::single();
    let (_window, _guard) = controls(&mut cx);
    let buttons = request(&mut cx, "query", json!({"query": "role=button"})).unwrap();
    let names: Vec<&str> = buttons["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Save", "Details", "Far"]);
    let wrap = &request(&mut cx, "query", json!({"query": "role=checkbox"})).unwrap()["nodes"][0];
    assert_eq!(wrap["checked"], "false");
    assert_eq!(wrap["bounds"]["width"], 20.);
    let bad = request(&mut cx, "query", json!({"query": "type=Div"})).unwrap_err();
    assert!(bad.message.contains("type="), "{bad}");
}
