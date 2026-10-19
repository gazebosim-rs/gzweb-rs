//! gz-transport topic names: a port of `gz::transport::TopicUtils`'s
//! `IsValidTopic` and `AsValidTopic` (gz-transport 15, Jetty), and the world
//! topics built with them.
//!
//! Gazebo sanitizes a world's name before it builds the world's topics
//! (`/world/<name>/…`), and Jetty's websocket server does the same to the
//! world name in a `scene` request. The `worlds` reply carries the raw SDF
//! name, so the client sanitizes it the same way. gzweb doesn't, so it gets
//! no scene, poses or control for a world whose name has, say, a space.
//! See DESIGN.md "Websocket protocol".

/// `TopicUtils::kMaxNameLength`, in bytes.
pub const MAX_NAME_LENGTH: usize = 65535;

/// `TopicUtils::IsValidTopic`: non-empty, at most [`MAX_NAME_LENGTH`] bytes,
/// not `/`, and without `~`, ` `, `//`, `@` or `:=`.
pub fn is_valid_topic(topic: &str) -> bool {
    !topic.is_empty()
        && topic.len() <= MAX_NAME_LENGTH
        && topic != "/"
        && !["~", " ", "//", "@", ":="]
            .iter()
            .any(|bad| topic.contains(bad))
}

/// `TopicUtils::AsValidTopic`: spaces become `_`; `@`, `~`, `:=` and `//`
/// are dropped (`//` as a pair, so `a//b` gives `ab`). `None` where upstream
/// returns an empty string: the result isn't a valid topic.
///
/// Upstream works on bytes; every byte it looks at is ASCII, which never
/// occurs inside a multi-byte UTF-8 sequence, so working on `char`s gives the
/// same bytes.
pub fn as_valid_topic(topic: &str) -> Option<String> {
    let mut out = String::with_capacity(topic.len());
    let mut chars = topic.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' => out.push('_'),
            '@' | '~' => {}
            ':' if chars.peek() == Some(&'=') => {
                chars.next();
            }
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
            }
            c => out.push(c),
        }
    }
    is_valid_topic(&out).then_some(out)
}

/// `/world/<world>/<leaf>`, sanitized as Jetty's websocket server builds its
/// `scene` service name: `AsValidTopic` over the whole topic. gz-sim's
/// systems sanitize `/world/<world>` and then append `/<leaf>`
/// (SceneBroadcaster: poses, scene info), some after sanitizing the name on
/// its own first (SimulationRunner: control, stats); all of these agree
/// unless the name contains a `/`. If no valid topic can be made, gz-sim
/// doesn't run the world, and this returns the topic unsanitized.
pub fn world_topic(world: &str, leaf: &str) -> String {
    let raw = format!("/world/{world}/{leaf}");
    as_valid_topic(&raw).unwrap_or(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Cases from gz-transport 15's `TopicUtils_TEST.cc` (`asValidTopic`).
    #[test]
    fn upstream_unmodified() {
        for t in [
            "/abc",
            "abc/de",
            "a",
            "ABC/",
            "/abc",
            "/abc/d",
            "/abc/d/e",
            "a(bc)d-e_f=h+i.j",
        ] {
            assert_eq!(as_valid_topic(t).as_deref(), Some(t));
            assert!(is_valid_topic(t), "{t}");
        }
    }

    #[test]
    fn upstream_modified() {
        for (t, want) in [
            ("a b  c", "a_b__c"),
            ("a@b@c", "abc"),
            ("a:=b:=c", "abc"),
            ("a//b/c", "ab/c"),
            ("a~b~c", "abc"),
        ] {
            let valid = as_valid_topic(t);
            assert_eq!(valid.as_deref(), Some(want), "{t}");
            assert!(is_valid_topic(want));
        }
    }

    #[test]
    fn upstream_fail() {
        for t in ["", "@@@", "~@~"] {
            assert_eq!(as_valid_topic(t), None, "{t}");
        }
    }

    // `IsValidTopic` cases from the same file (`testTopics`).
    #[test]
    fn upstream_validity() {
        for t in [
            "abc", "/abc", "abc/de", "a", "abc/", "/abc/", "/abc/d", "/abc/d/e",
        ] {
            assert!(is_valid_topic(t), "{t}");
        }
        for t in [
            "",
            " ",
            "~a",
            "topic1 ",
            "abc//def",
            "ab~cd",
            "/",
            "~/",
            "~",
            "@partition",
            "topic:=",
        ] {
            assert!(!is_valid_topic(t), "{t}");
        }
        assert!(is_valid_topic(&"a".repeat(MAX_NAME_LENGTH)));
        assert!(!is_valid_topic(&"a".repeat(MAX_NAME_LENGTH + 1)));
    }

    // Behaviour that follows from the upstream code.
    #[test]
    fn edge_cases() {
        // `//` is dropped as a pair, then the rest is scanned again: `///`
        // keeps one slash.
        assert_eq!(as_valid_topic("a///b").as_deref(), Some("a/b"));
        assert_eq!(as_valid_topic("a////b").as_deref(), Some("ab"));
        // A lone `:` or `=` stays; `::=` keeps the first `:`.
        assert_eq!(as_valid_topic("a:b=c").as_deref(), Some("a:b=c"));
        assert_eq!(as_valid_topic("a::=b").as_deref(), Some("a:b"));
        // Results that are still invalid: `/`, or `//` formed by a removal.
        assert_eq!(as_valid_topic("/@"), None);
        assert_eq!(as_valid_topic("/@/"), None);
        assert_eq!(as_valid_topic(" "), Some("_".into()));
        // Non-ASCII passes through.
        assert_eq!(as_valid_topic("wörld 1").as_deref(), Some("wörld_1"));
        // Too long after sanitizing.
        assert_eq!(as_valid_topic(&"a".repeat(MAX_NAME_LENGTH + 1)), None);
        assert!(as_valid_topic(&format!("{}@", "a".repeat(MAX_NAME_LENGTH))).is_some());
    }

    #[test]
    fn world_topics() {
        assert_eq!(
            world_topic("shapes", "scene/info"),
            "/world/shapes/scene/info"
        );
        assert_eq!(
            world_topic("my world@1", "dynamic_pose/info"),
            "/world/my_world1/dynamic_pose/info"
        );
        assert_eq!(world_topic("a:=b~", "control"), "/world/ab/control");
        // Nothing valid can be made: unsanitized.
        assert_eq!(
            world_topic(&"a".repeat(MAX_NAME_LENGTH), "stats"),
            format!("/world/{}/stats", "a".repeat(MAX_NAME_LENGTH))
        );
    }
}
