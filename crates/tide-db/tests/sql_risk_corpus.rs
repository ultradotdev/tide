//! Offline classifier expectations for the corpus that `live_postgres`
//! replays against a real server.

#[path = "support/sql_corpus.rs"]
mod sql_corpus;

#[test]
fn adversarial_corpus_requires_confirmation_and_names_operations() {
    for case in sql_corpus::ADVERSARIAL {
        sql_corpus::assert_classified(case);
    }
}

#[test]
fn benign_corpus_stays_safe() {
    for sql in sql_corpus::BENIGN {
        sql_corpus::assert_benign(sql);
    }
}
