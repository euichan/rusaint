//! 등록금 납부 확인서 (ZCMW6560) — 학생회비 납부 여부를 읽는다.
//!
//! 숭실 u-saint 의 '등록금 납부 확인서'는 OZ 리포트(`zcm_get_regist_check_paper_rfc`, category `CM`)로
//! 뜨며, 학생회비가 전용 라인으로 금액과 함께 찍힌다. 파이프라인은 [`CourseRegistrationStatusApplication`]
//! (ZCMW2110)과 동형: 학년도/학기 선택 → 조회 버튼 press → `openExternalWindow` OZ URL →
//! [`fetch_data_module`] → datasets.
//!
//! 납부확인서 폼은 포털에서 WebDynpro 앱 `ZCMW6560`으로 뜬다(고지서 출력은 별도 앱 ZCMW6530 — 혼동 주의).
//! ZCMW6560 의 element id(년/학기 콤보·조회 버튼)와 datasets 의 학생회비 필드명은 계정별 실측(recon)으로
//! 확정한다. [`RegistrationFeeApplication::recon`]이 한 번의 실행으로 그 둘을 함께 덤프한다.

use crate::ApplicationError;
use crate::application::registration_fee::model::StudentFeePayment;
use crate::application::utils::oz::{
    extract_oz_url_from_script_calls, fetch_data_module, parse_oz_url_params,
};
use crate::client::{USaintApplication, USaintClient};
use crate::{RusaintError, model::SemesterType};
use ozra::types::DataSet;
use serde::Serialize;
use wdpe::command::WebDynproCommandExecutor;
use wdpe::command::element::action::ButtonPressEventCommand;
use wdpe::command::element::selection::ComboBoxSelectEventCommand;
use wdpe::element::action::ButtonDef;
use wdpe::element::definition::ElementDefinition;
use wdpe::element::parser::ElementParser;
use wdpe::element::selection::ComboBoxDef;
use wdpe::error::WebDynproError;
use wdpe::scraper::Selector;
use wdpe::state::EventProcessResult;

/// [등록금 납부 확인서(ZCMW6560)](https://ecc.ssu.ac.kr/sap/bc/webdynpro/SAP/ZCMW6560)
#[derive(Debug)]
pub struct RegistrationFeeApplication {
    client: USaintClient,
}

impl USaintApplication for RegistrationFeeApplication {
    const APP_NAME: &'static str = "ZCMW6560";

    fn from_client(client: USaintClient) -> Result<Self, RusaintError> {
        if client.name() != Self::APP_NAME {
            Err(RusaintError::InvalidClientError)
        } else {
            Ok(Self { client })
        }
    }
}

fn semester_to_key(semester: SemesterType) -> &'static str {
    match semester {
        SemesterType::One => "090",
        SemesterType::Summer => "091",
        SemesterType::Two => "092",
        SemesterType::Winter => "093",
    }
}

/// recon 덤프의 단일 element (id, 컨트롤타입 ct, 주변 텍스트).
#[derive(Debug, Clone, Serialize)]
pub struct ReconElement {
    /// WebDynpro element id.
    pub id: String,
    /// 컨트롤 타입(`CB`=ComboBox, `B`=Button, `TS`=TabStrip 등).
    pub ct: Option<String>,
    /// element 내부 텍스트(라벨 식별용, 최대 40자).
    pub text: String,
}

/// OZ datasets 한 개의 구조 덤프 (학생회비 필드명을 찾기 위함).
#[derive(Debug, Serialize)]
pub struct ReconDataset {
    /// OZ 데이터셋 이름.
    pub name: String,
    /// 행 수(마스터 1행 vs 라인아이템 N행 판별용).
    pub row_count: usize,
    /// 앞 20행까지의 (필드명, 값). 어느 데이터셋·필드가 학생회비인지, 라인아이템이면 '학생회비' 행을
    /// 여기서 확정. (마스터 요약이면 1행, 라인아이템이면 여러 행에 입학금·수업료·학생회비·… 가 뜬다.)
    pub rows: Vec<Vec<(String, String)>>,
}

/// [`RegistrationFeeApplication::recon`]의 결과 — Ted 1회 실행으로 얻는 전부.
#[derive(Debug, Serialize)]
pub struct FeeRecon {
    /// 로드된 앱 이름(ZCMW6560).
    pub app_name: String,
    /// 덤프된 전체 element 수.
    pub total_elements: usize,
    /// `.PERYR`로 끝나는 ComboBox(학년도). 없으면 자체 기간 element.
    pub period_year_id: Option<String>,
    /// `.PERID`로 끝나는 ComboBox(학기).
    pub period_id_id: Option<String>,
    /// 모든 ComboBox 후보(학년도/학기 식별 교차검증용).
    pub combo_boxes: Vec<ReconElement>,
    /// 모든 Button 후보(조회 버튼 식별용).
    pub buttons: Vec<ReconElement>,
    /// openExternalWindow(OZ)를 띄운 조회 버튼 id. null이면 --button-id 로 재지정.
    pub button_used: Option<String>,
    /// 추출된 OZ 뷰어 URL.
    pub oz_url: Option<String>,
    /// OZ fetch 성공 시 datasets 구조(학생회비 필드 확정용).
    pub datasets: Vec<ReconDataset>,
    /// 전체 element(id, ct, text) — 납부확인서 뷰로 가는 네비게이션(링크/탭/메뉴) 탐색용.
    pub all_elements: Vec<ReconElement>,
    /// OZ fetch 가 에러로 실패했으면 그 메시지(빈 성공 vs 에러 구분용).
    pub fetch_error: Option<String>,
}

/// [디버그] 주어진 OZ 뷰어 URL 을 그대로 fetch 해 datasets 구조를 덤프한다.
/// P_RANDOM 이 유효한 동안만 동작(일회용 토큰). 세션 불필요(OZ guest/guest + URL 의 P_RANDOM).
pub async fn fetch_oz_url_debug(url: &str) -> Result<Vec<ReconDataset>, RusaintError> {
    let params = parse_oz_url_params(url)?;
    let response = fetch_data_module(&params, None).await?;
    Ok(dump_datasets(&response.datasets))
}

fn dump_datasets(datasets: &[DataSet]) -> Vec<ReconDataset> {
    datasets
        .iter()
        .map(|(name, rows)| ReconDataset {
            name: name.clone(),
            row_count: rows.len(),
            rows: rows
                .iter()
                .take(20)
                .map(|r| {
                    r.iter()
                        .map(|(k, v)| (k.clone(), v.to_string_repr()))
                        .collect()
                })
                .collect(),
        })
        .collect()
}

#[allow(unused)]
impl RegistrationFeeApplication {
    /// 현재 body 의 모든 element(id + ct + 텍스트)를 덤프한다.
    fn dump_elements(&self) -> Vec<ReconElement> {
        let parser = ElementParser::new(self.client.body());
        let selector = Selector::parse("[ct]").unwrap();
        parser
            .document()
            .select(&selector)
            .filter_map(|el| {
                let id = el.value().attr("id")?.to_string();
                if id.is_empty() {
                    return None;
                }
                let ct = el.value().attr("ct").map(|s| s.to_string());
                let text: String = el
                    .text()
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(40)
                    .collect();
                Some(ReconElement { id, ct, text })
            })
            .collect()
    }

    /// 학년도/학기 ComboBox 를 선택한다(동적 id).
    async fn select_period(
        &mut self,
        year_id: &str,
        period_id: &str,
        year: u32,
        semester: SemesterType,
    ) -> Result<(), WebDynproError> {
        let year_str = year.to_string();
        let parser = ElementParser::new(self.client.body());
        let year_event = parser.read(ComboBoxSelectEventCommand::new(
            ComboBoxDef::new_dynamic(year_id.to_string()),
            &year_str,
            false,
        ))?;
        self.client.process_event(false, year_event).await?;

        let parser = ElementParser::new(self.client.body());
        let semester_event = parser.read(ComboBoxSelectEventCommand::new(
            ComboBoxDef::new_dynamic(period_id.to_string()),
            semester_to_key(semester),
            false,
        ))?;
        self.client.process_event(false, semester_event).await?;
        Ok(())
    }

    /// 버튼을 눌러 `openExternalWindow` OZ URL 을 얻는다. 없으면 `None`.
    async fn press_for_oz(&mut self, button_id: &str) -> Result<Option<String>, RusaintError> {
        let parser = ElementParser::new(self.client.body());
        let press = parser.read(ButtonPressEventCommand::new(ButtonDef::new_dynamic(
            button_id.to_string(),
        )))?;
        let result = self.client.process_event(true, press).await?;
        let script_calls = match result {
            EventProcessResult::Sent(update) => update.script_calls.unwrap_or_default(),
            EventProcessResult::Enqueued => return Ok(None),
        };
        Ok(extract_oz_url_from_script_calls(&script_calls).ok())
    }

    /// OZ URL 에서 해당 학기 datasets 를 가져온다(arg1=year, arg2=semester 로 덮어씀).
    async fn fetch_datasets(
        &mut self,
        oz_url: &str,
        year: u32,
        semester: SemesterType,
        override_term: bool,
    ) -> Result<Vec<DataSet>, RusaintError> {
        let mut oz_params = parse_oz_url_params(oz_url)?;
        // override_term=true 는 ZCMW2110 패턴(arg1/arg2 를 요청 학기로 교체). 단 P_RANDOM 이 원래
        // 학기에 묶여 있으면 불일치로 빈 응답이 날 수 있다 — 기간을 WebDynpro 에서 먼저 선택해
        // URL 자체가 해당 학기로 뜬 경우엔 override_term=false(raw)로 그대로 fetch 한다.
        if override_term {
            let year_value = year.to_string();
            let semester_key = semester_to_key(semester).to_string();
            for (key, value) in &mut oz_params.params {
                if key == "arg1" {
                    *value = year_value.clone();
                } else if key == "arg2" {
                    *value = semester_key.clone();
                }
            }
        }
        let response = fetch_data_module(&oz_params, Some(self.client.http_client())).await?;
        Ok(response.datasets)
    }

    /// 기간을 선택하고 조회 버튼을 눌러 OZ URL 을 찾는다.
    /// `button_id` 가 없으면 모든 버튼을 순회하며 `openExternalWindow` 를 처음 띄우는 버튼을 자동 선정.
    /// 반환: (사용한 버튼 id, OZ URL).
    async fn resolve_oz_url(
        &mut self,
        year: u32,
        semester: SemesterType,
        button_id: Option<String>,
    ) -> Result<(Option<String>, Option<String>), RusaintError> {
        let all = self.dump_elements();
        let period_year_id = all
            .iter()
            .find(|e| e.ct.as_deref() == Some("CB") && e.id.ends_with(".PERYR"))
            .map(|e| e.id.clone());
        let period_id_id = all
            .iter()
            .find(|e| e.ct.as_deref() == Some("CB") && e.id.ends_with(".PERID"))
            .map(|e| e.id.clone());

        // 기간 콤보를 못 찾으면(접미사 다른 레이아웃) 학기 선택을 건너뛰게 되는데, 그러면 OZ 가
        // 기본 학기 기준으로 뜰 수 있다 — 조용히 넘기지 말고 경고를 남긴다(recon 이 접미사 확정).
        if period_year_id.is_none() || period_id_id.is_none() {
            tracing::warn!(
                "기간 콤보 미발견(PERYR={:?}, PERID={:?}) — 학기 선택 생략, 기본 학기로 뜰 수 있음",
                period_year_id,
                period_id_id
            );
        }
        if let (Some(py), Some(pid)) = (&period_year_id, &period_id_id) {
            let _ = self.select_period(py, pid, year, semester).await;
        }

        // button_id 미지정 시 '조회/출력' 라벨 버튼만 누른다 — '분할납부신청' 등 상태변경 버튼을
        // 건드리지 않기 위함(납부확인서 폼에도 분할납부신청 등 상태변경 버튼이 함께 있다). 라벨이 비면
        // (아이콘 전용) 아무것도 누르지 않고 덤프만 돌려줘 --button-id 로 재지정하게 한다.
        let candidates: Vec<String> = match &button_id {
            Some(b) => vec![b.clone()],
            None => all
                .iter()
                .filter(|e| {
                    e.ct.as_deref() == Some("B")
                        && (e.text.contains("조회") || e.text.contains("출력"))
                })
                .map(|e| e.id.clone())
                .collect(),
        };
        for bid in candidates {
            match self.press_for_oz(&bid).await {
                Ok(Some(url)) => return Ok((Some(bid), Some(url))),
                Ok(None) => {}
                // press 실패를 조용히 버리지 않는다 — 다음 후보로 넘어가되 로그에 남긴다.
                Err(e) => tracing::debug!("조회 버튼 press 실패({bid}): {e}"),
            }
            let _ = self.client.reload().await;
            if let (Some(py), Some(pid)) = (&period_year_id, &period_id_id) {
                let _ = self.select_period(py, pid, year, semester).await;
            }
        }
        Ok((None, None))
    }

    /// [recon] element 트리 + OZ datasets 구조를 한 번에 덤프한다(계정별 실측 1회용).
    ///
    /// `button_id`가 없으면 모든 버튼을 순회하며 `openExternalWindow`를 처음 띄우는 버튼을 자동 선정한다.
    pub async fn recon(
        &mut self,
        year: u32,
        semester: SemesterType,
        button_id: Option<String>,
    ) -> Result<FeeRecon, RusaintError> {
        let all = self.dump_elements();
        let period_year_id = all
            .iter()
            .find(|e| e.ct.as_deref() == Some("CB") && e.id.ends_with(".PERYR"))
            .map(|e| e.id.clone());
        let period_id_id = all
            .iter()
            .find(|e| e.ct.as_deref() == Some("CB") && e.id.ends_with(".PERID"))
            .map(|e| e.id.clone());
        let combo_boxes: Vec<ReconElement> = all
            .iter()
            .filter(|e| e.ct.as_deref() == Some("CB"))
            .cloned()
            .collect();
        let buttons: Vec<ReconElement> = all
            .iter()
            .filter(|e| e.ct.as_deref() == Some("B"))
            .cloned()
            .collect();

        let (button_used, oz_url) = self.resolve_oz_url(year, semester, button_id).await?;

        let mut fetch_error = None;
        let datasets = match &oz_url {
            Some(url) => match self.fetch_datasets(url, year, semester, false).await {
                Ok(ds) => dump_datasets(&ds),
                Err(e) => {
                    fetch_error = Some(e.to_string());
                    vec![]
                }
            },
            None => vec![],
        };

        Ok(FeeRecon {
            app_name: Self::APP_NAME.to_string(),
            total_elements: all.len(),
            period_year_id,
            period_id_id,
            combo_boxes,
            buttons,
            button_used,
            oz_url,
            datasets,
            all_elements: all,
            fetch_error,
        })
    }

    /// [운영] 해당 학기의 학생회비 납부 정보를 조회한다(조회 버튼 자동 탐색).
    pub async fn fee(
        &mut self,
        year: u32,
        semester: SemesterType,
    ) -> Result<StudentFeePayment, RusaintError> {
        let (_, oz_url) = self.resolve_oz_url(year, semester, None).await?;
        let oz_url = oz_url.ok_or_else(|| {
            ApplicationError::OzDataFetchError(
                "조회 버튼에서 OZ URL(openExternalWindow)을 얻지 못했습니다".to_string(),
            )
        })?;
        let datasets = self.fetch_datasets(&oz_url, year, semester, false).await?;
        let fee = StudentFeePayment::from_datasets(&datasets)?;

        // 기간 선택이 조용히 실패하면(콤보 거부·일시 오류·미검증 학기) 폼이 기본(현재) 학기에 머물러
        // '다른 학기 납부여부'를 요청 학기인 척 돌려줄 수 있다 — 응답이 가리키는 학기를 요청과 대조해
        // 어긋나면 잘못된 paid 를 내지 말고 에러로 올린다.
        if let Some(reason) = fee.term_mismatch(year, semester) {
            return Err(ApplicationError::OzDataFetchError(format!(
                "조회 학기 불일치({reason}) — 기간 선택이 반영되지 않았습니다"
            ))
            .into());
        }
        Ok(fee)
    }

    /// 페이지를 새로고침합니다.
    pub async fn reload(&mut self) -> Result<(), RusaintError> {
        self.client.reload().await?;
        Ok(())
    }
}

/// [`RegistrationFeeApplication`]에서 사용하는 데이터
pub mod model;
