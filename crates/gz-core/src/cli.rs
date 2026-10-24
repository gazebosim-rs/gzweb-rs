//! The Console's command line (spec §3.3: "gz command… e.g. service -s
//! /world/…"): the `gz topic` and `gz service` commands the websocket
//! server can carry, read from what the user typed, with the same flags as
//! the `gz` tool. Running them is the Console's.

/// A command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GzCli {
    Help,
    /// `topic -l`
    TopicList,
    /// `topic -i -t TOPIC`: its message type.
    TopicInfo {
        topic: String,
    },
    /// `topic -e -t TOPIC`
    TopicEcho {
        topic: String,
    },
    /// `topic -t TOPIC -m TYPE -p MSG`, the message in text format.
    TopicPub {
        topic: String,
        msg_type: String,
        text: String,
    },
    /// `service -l`
    ServiceList,
    /// `service -s SERVICE --reqtype TYPE --reptype TYPE [--timeout MS]
    /// [--req MSG]`, the request in text format (empty: the empty
    /// message).
    ServiceCall {
        service: String,
        req_type: String,
        rep_type: String,
        timeout_ms: u32,
        req: String,
    },
}

/// How long a service call waits for its reply when `--timeout` doesn't
/// say (longer than `gz`'s 1 s: the websocket adds a round trip, and a
/// `create` from Fuel downloads first).
pub const DEFAULT_TIMEOUT_MS: u32 = 5000;

/// The commands, one a line.
pub const HELP: &str = "topic -l
topic -i -t TOPIC
topic -e -t TOPIC
topic -t TOPIC -m TYPE -p MSG
service -s SERVICE --reqtype TYPE --reptype TYPE [--timeout MS] [--req MSG]";

const TOPIC_USAGE: &str = "usage: topic -l | -i -t TOPIC | -e -t TOPIC | -t TOPIC -m TYPE -p MSG";
const SERVICE_USAGE: &str =
    "usage: service -s SERVICE --reqtype TYPE --reptype TYPE [--timeout MS] [--req MSG]";

/// Why a line isn't a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliError {
    /// Nothing but spaces.
    Empty,
    /// A quote isn't closed.
    Quote,
    /// Not `topic`, `service` or `help`.
    Unknown(String),
    /// A flag the command doesn't take, or one missing its value.
    Usage(&'static str),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Empty => write!(f, "no command"),
            CliError::Quote => write!(f, "a quote isn't closed"),
            CliError::Unknown(c) => write!(f, "unknown command {c}: try help"),
            CliError::Usage(u) => write!(f, "{u}"),
        }
    }
}

impl std::error::Error for CliError {}

/// `line` split into words as a shell does: on spaces, except inside
/// single or double quotes; a backslash keeps the next character (outside
/// single quotes).
pub fn words(line: &str) -> Result<Vec<String>, CliError> {
    let mut out = vec![];
    let mut word: Option<String> = None;
    let mut quote = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('\''), c) => word.get_or_insert_default().push(c),
            (_, '\\') => {
                let next = chars.next().unwrap_or('\\');
                word.get_or_insert_default().push(next);
            }
            (None, '\'' | '"') => {
                quote = Some(c);
                word.get_or_insert_default();
            }
            (None, c) if c.is_whitespace() => out.extend(word.take()),
            (_, c) => word.get_or_insert_default().push(c),
        }
    }
    if quote.is_some() {
        return Err(CliError::Quote);
    }
    out.extend(word);
    Ok(out)
}

/// The flags given: each `-x VALUE`, `--long VALUE` or `--long=VALUE`,
/// and the bare ones.
struct Flags {
    values: Vec<(String, String)>,
    bare: Vec<String>,
}

impl Flags {
    /// Reads `args`: a flag in `takes` takes the next word as its value.
    fn read(
        args: &[String],
        takes: &[&str],
        bare: &[&str],
        usage: &'static str,
    ) -> Result<Flags, CliError> {
        let mut flags = Flags {
            values: vec![],
            bare: vec![],
        };
        let mut i = 0;
        while i < args.len() {
            let (name, inline) = match args[i].split_once('=') {
                Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
                _ => (args[i].as_str(), None),
            };
            if takes.contains(&name) {
                let value = match inline {
                    Some(v) => v,
                    None => {
                        i += 1;
                        args.get(i).cloned().ok_or(CliError::Usage(usage))?
                    }
                };
                flags.values.push((name.to_string(), value));
            } else if bare.contains(&name) && inline.is_none() {
                flags.bare.push(name.to_string());
            } else {
                return Err(CliError::Usage(usage));
            }
            i += 1;
        }
        Ok(flags)
    }

    /// The value of the flag named `short` or `long`, if given.
    fn get(&self, short: &str, long: &str) -> Option<String> {
        self.values
            .iter()
            .rev()
            .find(|(n, _)| n == short || n == long)
            .map(|(_, v)| v.clone())
    }

    fn has(&self, short: &str, long: &str) -> bool {
        self.bare.iter().any(|n| n == short || n == long)
    }
}

/// `line` as a command; a leading `gz` is optional.
pub fn parse(line: &str) -> Result<GzCli, CliError> {
    let words = words(line)?;
    let mut args = words.as_slice();
    if args.first().is_some_and(|w| w == "gz") {
        args = &args[1..];
    }
    let Some((command, rest)) = args.split_first() else {
        return Err(CliError::Empty);
    };
    match command.as_str() {
        "help" | "-h" | "--help" => Ok(GzCli::Help),
        "topic" => topic(rest),
        "service" => service(rest),
        other => Err(CliError::Unknown(other.to_string())),
    }
}

fn topic(args: &[String]) -> Result<GzCli, CliError> {
    let usage = TOPIC_USAGE;
    let f = Flags::read(
        args,
        &["-t", "--topic", "-m", "--msgtype", "-p", "--pub"],
        &["-l", "--list", "-i", "--info", "-e", "--echo"],
        usage,
    )?;
    let topic = f.get("-t", "--topic");
    let need = |t: Option<String>| t.ok_or(CliError::Usage(usage));
    if f.has("-l", "--list") {
        return Ok(GzCli::TopicList);
    }
    if f.has("-i", "--info") {
        return Ok(GzCli::TopicInfo {
            topic: need(topic)?,
        });
    }
    if f.has("-e", "--echo") {
        return Ok(GzCli::TopicEcho {
            topic: need(topic)?,
        });
    }
    match (topic, f.get("-m", "--msgtype"), f.get("-p", "--pub")) {
        (Some(topic), Some(msg_type), Some(text)) => Ok(GzCli::TopicPub {
            topic,
            msg_type,
            text,
        }),
        _ => Err(CliError::Usage(usage)),
    }
}

fn service(args: &[String]) -> Result<GzCli, CliError> {
    let usage = SERVICE_USAGE;
    let f = Flags::read(
        args,
        &[
            "-s",
            "--service",
            "--reqtype",
            "--reptype",
            "--timeout",
            "--req",
            "-r",
        ],
        &["-l", "--list"],
        usage,
    )?;
    if f.has("-l", "--list") {
        return Ok(GzCli::ServiceList);
    }
    let need = |v: Option<String>| v.ok_or(CliError::Usage(usage));
    let timeout_ms = match f.get("--timeout", "--timeout") {
        Some(t) => t.parse().map_err(|_| CliError::Usage(usage))?,
        None => DEFAULT_TIMEOUT_MS,
    };
    Ok(GzCli::ServiceCall {
        service: need(f.get("-s", "--service"))?,
        req_type: need(f.get("--reqtype", "--reqtype"))?,
        rep_type: need(f.get("--reptype", "--reptype"))?,
        timeout_ms,
        req: f.get("-r", "--req").unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Words split on spaces, quotes keep theirs, a backslash escapes, an
    /// open quote is an error.
    #[test]
    fn words_split_as_a_shell_does() {
        assert_eq!(
            words(r#"topic -p 'linear: { x: 1 }' "a b" c\ d"#).unwrap(),
            ["topic", "-p", "linear: { x: 1 }", "a b", "c d"]
        );
        assert_eq!(words(r#"-p 'it\s' """#).unwrap(), ["-p", r"it\s", ""]);
        assert_eq!(words("   ").unwrap(), Vec::<String>::new());
        assert_eq!(words("'open"), Err(CliError::Quote));
    }

    /// The `gz topic` and `gz service` forms, with or without `gz`, short
    /// or long flags, `--flag=value`; what's wrong says how to fix it.
    #[test]
    fn commands_parse_as_gz_reads_them() {
        assert_eq!(parse("gz topic -l"), Ok(GzCli::TopicList));
        assert_eq!(parse("topic --list"), Ok(GzCli::TopicList));
        assert_eq!(
            parse("topic -i -t /clock"),
            Ok(GzCli::TopicInfo {
                topic: "/clock".into()
            })
        );
        assert_eq!(
            parse("topic -e --topic=/stats"),
            Ok(GzCli::TopicEcho {
                topic: "/stats".into()
            })
        );
        assert_eq!(
            parse("topic -t /cmd_vel -m gz.msgs.Twist -p 'linear: { x: 0.5 }'"),
            Ok(GzCli::TopicPub {
                topic: "/cmd_vel".into(),
                msg_type: "gz.msgs.Twist".into(),
                text: "linear: { x: 0.5 }".into()
            })
        );
        assert_eq!(
            parse(
                "gz service -s /world/w/control --reqtype gz.msgs.WorldControl \
                 --reptype gz.msgs.Boolean --timeout 300 --req 'pause: true'"
            ),
            Ok(GzCli::ServiceCall {
                service: "/world/w/control".into(),
                req_type: "gz.msgs.WorldControl".into(),
                rep_type: "gz.msgs.Boolean".into(),
                timeout_ms: 300,
                req: "pause: true".into()
            })
        );
        assert!(matches!(
            parse("service -s /a --reqtype A --reptype B"),
            Ok(GzCli::ServiceCall { timeout_ms: DEFAULT_TIMEOUT_MS, ref req, .. }) if req.is_empty()
        ));
        assert_eq!(parse("service --list"), Ok(GzCli::ServiceList));
        assert_eq!(parse("help"), Ok(GzCli::Help));

        assert_eq!(parse(""), Err(CliError::Empty));
        assert_eq!(parse("gz"), Err(CliError::Empty));
        assert_eq!(parse("model -l"), Err(CliError::Unknown("model".into())));
        assert_eq!(parse("topic -e"), Err(CliError::Usage(TOPIC_USAGE)));
        assert_eq!(parse("topic -t"), Err(CliError::Usage(TOPIC_USAGE)));
        assert_eq!(parse("topic -x"), Err(CliError::Usage(TOPIC_USAGE)));
        assert_eq!(parse("topic -t /a -m T"), Err(CliError::Usage(TOPIC_USAGE)));
        assert_eq!(
            parse("service -s /a --reqtype A --reptype B --timeout soon"),
            Err(CliError::Usage(SERVICE_USAGE))
        );
        assert_eq!(parse("service -s /a"), Err(CliError::Usage(SERVICE_USAGE)));
        assert_eq!(
            CliError::Unknown("model".into()).to_string(),
            "unknown command model: try help"
        );
        assert_eq!(CliError::Quote.to_string(), "a quote isn't closed");
        assert_eq!(CliError::Empty.to_string(), "no command");
        assert!(CliError::Usage(TOPIC_USAGE)
            .to_string()
            .starts_with("usage: topic"));
    }
}
