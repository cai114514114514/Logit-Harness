use loh::{domain::EventData, runtime::state::Session, store::Store};

pub fn assert_user_turns(session: &Session, store: &Store) {
    for event in store.events(&session.id, 0).unwrap() {
        if let EventData::Identity { actor, turn, .. } = event.data {
            assert!(
                session.turns.iter().any(|source| source.id == turn),
                "{actor} references absent user turn {turn}"
            );
        }
    }
}
