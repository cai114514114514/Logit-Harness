//! A local, explicitly synthetic saved conversation for UI review. No model call.
use loh::{
    config::Paths,
    domain::{EventData, Status, Usage},
    runtime::state::Session,
    store::Store,
};
use serde_json::json;

fn main() -> anyhow::Result<()> {
    let home = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: terminal_preview STATE_DIRECTORY"))?;
    let paths = Paths::new(home.into());
    paths.prepare()?;
    let mut session = Session::new(
        std::env::current_dir()?,
        "把 JSON 存储改成 SQLite，保留 CLI 行为与现有用户数据。",
        5_000_000,
        21600,
    );
    session.status = Status::Paused;
    session.budget.reported = Usage {
        input: 25000,
        output: 2000,
        cached: 21000,
    };
    session.budget.used = 27000;
    session.elapsed = 42;
    let store = Store::open(&paths.database())?;
    let events = vec![
        EventData::Created { objective: session.goal.original.clone(), workspace: session.workspace.display().to_string() },
        EventData::Message { actor: "main".into(), text: "我会先检查 **存储接口** 和现有测试，再实现 SQLite 迁移。命令行参数和返回格式会保留。".into() },
        EventData::ActionStarted { id: "read".into(), tool: "read_file".into(), arguments: json!({"path":"src/storage.rs"}) },
        EventData::ActionFinished { id: "read".into(), success: true, result: json!({"result":{"data":{"content":"pub trait Store {\n    fn get(&self, key: &str) -> Result<Option<String>>;\n    fn set(&mut self, key: &str, value: &str) -> Result<()>;\n}"}}}) },
        EventData::User { message: "已有 JSON 数据要自动迁移，重复启动不能重复导入。".into() },
        EventData::Revised { revision: 2, changed: vec!["存储迁移".into()] },
        EventData::ActionStarted { id: "test".into(), tool: "check".into(), arguments: json!({"command":"cargo test storage::tests"}) },
        EventData::ActionFinished { id: "test".into(), success: false, result: json!({"result":{"data":{"process":{"output":"running 4 tests\ntest preserves_cli ... ok\ntest migrates_existing_data ... ok\ntest migration_is_idempotent ... FAILED\nassertion failed: expected 3 rows, found 6"}}}}) },
        EventData::Message { actor: "main".into(), text: "测试发现重复导入：第二次启动又写入了三条记录。接下来会在事务中记录迁移标记，再重跑测试。\n\n```rust\ntransaction.commit()?;\n```\n这条记录是 **离线 UI 演示**，不是实际模型验收结果。".into() },
    ];
    for event in events {
        store.commit(&session.id, &session, event)?;
    }
    println!("{}", session.id);
    Ok(())
}
