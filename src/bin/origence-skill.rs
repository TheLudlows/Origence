use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = origence::skill_workflow::SkillCli::parse();
    match cli.run().await {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value)?),
        Err(error) => anyhow::bail!("{}: {}", error.code(), error.safe_message()),
    }
    Ok(())
}
