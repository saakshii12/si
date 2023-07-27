use crate::{CliResult, SiCliError};
use comfy_table::presets::UTF8_FULL;
use comfy_table::*;
use docker_api::Docker;
use si_posthog::PosthogClient;

pub async fn invoke(
    posthog_client: &PosthogClient,
    mode: String,
    silent: bool,
    is_preview: bool,
) -> CliResult<()> {
    let _ = posthog_client.capture(
        "si-command",
        "sally@systeminit.com",
        serde_json::json!({"name": "check-dependencies", "mode": mode}),
    );

    if !silent {
        println!("Checking that the system is able to interact with the docker engine to control System Initiative...");
    }

    if is_preview {
        return Ok(());
    }

    let docker = Docker::unix("//var/run/docker.sock");
    if let Err(_e) = docker.ping().await {
        return Err(SiCliError::DockerEngine);
    }

    if !silent {
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_width(100)
            .add_row(vec![
                Cell::new("Docker Engine Active").add_attribute(Attribute::Bold),
                Cell::new("    ✅    "),
            ]);

        println!("{table}");
    }

    Ok(())
}
