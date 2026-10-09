use std::{path::Path, sync::Arc};

use clap::Subcommand;
use rusaint::{
    USaintSession, application::registration_fee::RegistrationFeeApplication,
    client::USaintClientBuilder,
};

use crate::{
    output::{OutputFormat, write_output},
    types::SemesterType,
};

#[derive(Subcommand)]
pub enum RegistrationFeeCommands {
    /// [recon] 납부확인서(ZCMW6560) element 트리 + OZ datasets 구조를 한 번에 덤프
    Recon {
        #[arg(short = 'y', long)]
        year: u32,
        #[arg(short = 's', long)]
        semester: SemesterType,
        /// 조회 버튼 element id 지정(자동 탐색 실패 시)
        #[arg(long)]
        button_id: Option<String>,
    },
    /// 학생회비 납부 조회(조회 버튼 자동 탐색)
    Check {
        #[arg(short = 'y', long)]
        year: u32,
        #[arg(short = 's', long)]
        semester: SemesterType,
    },
    /// [디버그] 주어진 OZ URL 을 그대로 fetch(P_RANDOM 유효 동안, 세션 불필요)
    FetchOz {
        #[arg(long)]
        url: String,
    },
}

pub async fn execute(
    session: Arc<USaintSession>,
    command: RegistrationFeeCommands,
    format: &OutputFormat,
    output: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    // FetchOz 는 세션/앱 로드 불필요 — OZ URL 을 바로 fetch(P_RANDOM 유효 동안).
    if let RegistrationFeeCommands::FetchOz { url } = &command {
        let datasets = rusaint::application::registration_fee::fetch_oz_url_debug(url).await?;
        write_output(format, output, &datasets)?;
        return Ok(());
    }

    let mut app = USaintClientBuilder::new()
        .session(session)
        .build_into::<RegistrationFeeApplication>()
        .await?;

    match command {
        RegistrationFeeCommands::Recon {
            year,
            semester,
            button_id,
        } => {
            let recon = app.recon(year, *semester, button_id).await?;
            write_output(format, output, &recon)?;
        }
        RegistrationFeeCommands::Check { year, semester } => {
            let fee = app.fee(year, *semester).await?;
            write_output(format, output, &fee)?;
        }
        RegistrationFeeCommands::FetchOz { .. } => unreachable!("FetchOz handled above"),
    }

    Ok(())
}
