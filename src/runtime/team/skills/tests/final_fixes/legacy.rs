use super::*;

#[tokio::test]
async fn legacy_worker_is_bound_before_first_live_output_and_usage() {
    let root = tempfile::tempdir().unwrap();
    let (runner, request, _) = fixture(root.path());
    let actor = runner.session.team.members[request.target.member_id()]
        .actor
        .clone()
        .unwrap();
    let mut old = serde_json::to_value(&runner.session).unwrap();
    old.as_object_mut().unwrap().remove("team");
    for actor in old["actors"].as_object_mut().unwrap().values_mut() {
        actor.as_object_mut().unwrap().remove("target");
    }
    old["actors"]["main"]["phase"] = serde_json::json!("done");
    let session = serde_json::from_value(old).unwrap();
    let (send, mut requests) = mpsc::unbounded_channel();
    let (mut runner, _, _) = Runner::new(
        session,
        runner.store,
        Arc::new(Capture(send)),
        runner.tools,
        Settings {
            max_output: 128,
            ..Settings::default()
        },
    );
    runner.session.budget.limit = 100_000;
    runner.schedule().await.unwrap();
    let (_, _, release) = requests.recv().await.unwrap();
    release.send(answer()).unwrap();
    settle(&mut runner).await;
    let events = runner.store.events(&runner.session.id, 0).unwrap();
    let binding = events.iter().position(|e| matches!(&e.data, EventData::ActorBound { actor: id, target, .. } if id == &actor && target.member_id() == actor)).unwrap();
    for position in events
        .iter()
        .enumerate()
        .filter_map(|(position, e)| match &e.data {
            EventData::Message { actor: id, .. } | EventData::Usage { actor: id, .. }
                if id == &actor =>
            {
                Some(position)
            }
            _ => None,
        })
    {
        assert!(binding < position);
    }
    assert!(events.iter().any(|e| matches!(&e.data, EventData::Usage { actor: id, input: 7, output: 3, .. } if id == &actor)));
    assert!(events.iter().any(|e| matches!(&e.data, EventData::Message { actor: id, text } if id == &actor && text == "settled answer")));
    assert_eq!(
        runner.session.actor_bindings()[&actor].target.member_id(),
        actor
    );
}
