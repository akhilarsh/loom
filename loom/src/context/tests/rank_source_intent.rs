//! Query-intent behaviour of [`crate::context::rank_source::rank_source`].

use super::source_fixtures::{full_node, graph};
use crate::context::config::RetrievalConfig;
use crate::context::rank::RankQuery;
use crate::context::rank_source::intent::{classify, QueryIntent, RelationDirection};
use crate::context::rank_source::rank_source;
use crate::context::schema::{SelectionReason, TextSearchHint};

#[test]
fn plain_prose_admits_no_symbol_question() {
    let fixture = graph(vec![(
        "src/files.rs",
        vec![full_node(
            "src/files.rs#function:read_files",
            "src/files.rs",
            &["read_files"],
            "fn read_files()",
        )],
    )]);
    let query = RankQuery {
        text: "read the remaining knowledge files".to_string(),
        ..RankQuery::default()
    };

    let ranked = rank_source(&query, &fixture, &RetrievalConfig::default());

    assert!(
        ranked
            .iter()
            .all(|candidate| !candidate.reasons.contains(&SelectionReason::SymbolQuestion)),
        "plain prose must not read as a symbol question: {ranked:#?}"
    );
}

#[test]
fn symbol_questions_name_the_whole_symbol_as_written() {
    let phrasings = [
        ("what does tokenize do", "tokenize"),
        ("What is Parser?", "Parser"),
        ("where is `Lexer::tokenize` defined", "Lexer::tokenize"),
        ("where's render_frame", "render_frame"),
        ("how does ContextPack work", "ContextPack"),
        ("how does rank_source works?", "rank_source"),
        ("explain fuse", "fuse"),
        ("show me config.load", "config.load"),
        (
            "where is build_pack_request implemented",
            "build_pack_request",
        ),
        ("what is `QueryIntent`", "QueryIntent"),
        ("  WHAT DOES tokenize DO?  ", "tokenize"),
        ("explain `Foo::bar`?", "Foo::bar"),
        ("what does parse does", "parse"),
    ];

    for (query, symbol) in phrasings {
        assert_eq!(
            classify(query),
            QueryIntent::SymbolQuestion {
                symbol: symbol.to_string(),
            },
            "{query:?}"
        );
    }
}

/// Every phrasing here either is not a symbol question or names no node in
/// the fixture, whose names are all one lowercase word the ordinary candidacy
/// rule refuses: so nothing at all may be admitted.
#[test]
fn negative_phrasings_admit_nothing() {
    let path = "src/engine_room.rs";
    let node = |name: &str| {
        full_node(
            &format!("{path}#function:{name}"),
            path,
            &[name],
            &format!("fn {name}()"),
        )
    };
    let fixture = graph(vec![(
        path,
        vec![node("tokenize"), node("render"), node("plan")],
    )]);
    let phrasings = [
        "what does the plan say",
        "what does parser do",
        "please tokenize the input before parsing",
        "what does tokenize do with tabs",
        "where is \"x\"",
        "explain tokenize to me",
        "show me how render works",
        "how does rendering work",
        "where is tokenize used in the tests",
        "can you explain render",
        "what's plan",
        "how does the renderer work",
    ];

    for text in phrasings {
        let query = RankQuery {
            text: text.to_string(),
            ..RankQuery::default()
        };
        let ranked = rank_source(&query, &fixture, &RetrievalConfig::default());
        assert!(
            ranked.is_empty(),
            "{text:?} ({:?}) admitted {ranked:#?}",
            classify(text)
        );
    }
}

/// Every relationship phrasing the classifier must name, with its direction and symbol.
fn relationship_phrasings() -> Vec<(&'static str, RelationDirection, &'static str)> {
    vec![
        ("who calls tokenize", RelationDirection::Callers, "tokenize"),
        (
            "What invokes `Lexer::next`?",
            RelationDirection::Callers,
            "Lexer::next",
        ),
        (
            "list the callers of render",
            RelationDirection::Callers,
            "render",
        ),
        ("callees of render", RelationDirection::Callees, "render"),
        (
            "what does render call?",
            RelationDirection::Callees,
            "render",
        ),
        (
            "who references Config",
            RelationDirection::References,
            "Config",
        ),
        (
            "what uses config.load",
            RelationDirection::References,
            "config.load",
        ),
        ("usages of Config", RelationDirection::References, "Config"),
        (
            "impact of changing Config",
            RelationDirection::Impact,
            "Config",
        ),
        (
            "what is the impact of `Config`",
            RelationDirection::Impact,
            "Config",
        ),
    ]
}

#[test]
fn relationship_phrasings_name_each_direction() {
    for (query, direction, symbol) in relationship_phrasings() {
        assert_eq!(
            classify(query),
            QueryIntent::Relationship {
                direction,
                symbol: symbol.to_string(),
            },
            "{query:?}"
        );
    }
}

#[test]
fn a_quoted_phrase_is_literal_but_a_quoted_name_or_short_word_is_not() {
    assert_eq!(
        classify("find \"it's broken\" in the logs"),
        QueryIntent::Literal {
            text: "it's broken".to_string(),
        }
    );
    assert_eq!(
        classify("grep for \"a-b\""),
        QueryIntent::Literal {
            text: "a-b".to_string(),
        }
    );
    assert_eq!(classify("where is \"x\""), QueryIntent::General);
    assert_eq!(classify("who calls \"parse_tokens\""), QueryIntent::General);
}

#[test]
fn literal_is_checked_before_relationship_and_symbol_question() {
    assert_eq!(
        classify("who calls target when \"the socket closes\""),
        QueryIntent::Literal {
            text: "the socket closes".to_string(),
        }
    );
    assert_eq!(
        classify("what does \"a b\" do"),
        QueryIntent::Literal {
            text: "a b".to_string(),
        }
    );
    assert_eq!(
        classify("what does render call"),
        QueryIntent::Relationship {
            direction: RelationDirection::Callees,
            symbol: "render".to_string(),
        },
        "a relationship is checked before the symbol question it resembles"
    );
}

#[test]
fn the_text_search_command_escapes_every_single_quote() {
    assert_eq!(
        TextSearchHint::for_pattern("connection refused").command,
        "rg -n -F -- 'connection refused'"
    );
    assert_eq!(
        TextSearchHint::for_pattern("it's").command,
        r"rg -n -F -- 'it'\''s'"
    );
    let hint = TextSearchHint::for_pattern("'quoted'");
    assert_eq!(hint.command, r"rg -n -F -- ''\''quoted'\'''");
    assert_eq!(hint.pattern, "'quoted'");
}

#[test]
fn a_quoted_span_with_a_newline_is_not_literal() {
    assert_eq!(
        classify("find \"connection\nrefused\" in the logs"),
        QueryIntent::General
    );
    assert_eq!(
        classify("find \"connection\nrefused\" or \"timed out\""),
        QueryIntent::Literal {
            text: "timed out".to_string(),
        },
        "the next span is still considered"
    );
}

#[test]
fn a_quoted_span_past_the_length_cap_is_not_literal() {
    let longest = format!("a {}", "b".repeat(118));
    let too_long = format!("a {}", "b".repeat(119));
    assert_eq!(longest.chars().count(), 120);
    assert_eq!(
        classify(&format!("find \"{longest}\"")),
        QueryIntent::Literal { text: longest }
    );
    assert_eq!(
        classify(&format!("find \"{too_long}\"")),
        QueryIntent::General
    );
}

#[test]
fn a_pronoun_names_no_symbol() {
    for text in [
        "how does it work",
        "what does this do",
        "what is That",
        "who calls it",
        "what does them call",
    ] {
        assert_eq!(classify(text), QueryIntent::General, "{text:?}");
    }
}

#[test]
fn a_pronoun_first_match_does_not_hide_a_later_symbol() {
    assert_eq!(
        classify("who calls it, and who calls parse_tokens"),
        QueryIntent::Relationship {
            direction: RelationDirection::Callers,
            symbol: "parse_tokens".to_string(),
        }
    );
    assert_eq!(
        classify("who uses this or who uses `Lexer::next`?"),
        QueryIntent::Relationship {
            direction: RelationDirection::References,
            symbol: "Lexer::next".to_string(),
        }
    );
}

#[test]
fn an_article_names_no_symbol() {
    for text in [
        "who calls the parser",
        "what uses a cache",
        "who references an entry",
        "What invokes THE lexer",
        "explain the",
        "what is a",
    ] {
        assert_eq!(classify(text), QueryIntent::General, "{text:?}");
    }
}

#[test]
fn an_article_admits_no_node_named_like_it() {
    let path = "src/letters.rs";
    let node = |name: &str| {
        full_node(
            &format!("{path}#function:{name}"),
            path,
            &[name],
            &format!("fn {name}()"),
        )
    };
    let fixture = graph(vec![(path, vec![node("a"), node("the"), node("an")])]);

    for text in ["what uses a cache", "who calls the parser"] {
        let query = RankQuery {
            text: text.to_string(),
            ..RankQuery::default()
        };
        let ranked = rank_source(&query, &fixture, &RetrievalConfig::default());
        assert!(ranked.is_empty(), "{text:?} admitted {ranked:#?}");
    }
}
