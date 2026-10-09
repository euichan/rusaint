use ozra::types::{DataSet, Row};
use serde::{Deserialize, Serialize};

use crate::model::SemesterType;
use crate::{ApplicationError, RusaintError};

/// 등록금 납부확인서(ZCMW6560 / OZ `zcm_get_regist_check_paper_rfc`)에서 읽은 학생회비 납부 정보.
///
/// 숭실은 학생회비를 등록금에 동반징수(선택납부)하며, 납부확인서 OZ 에 '학생회비'가 전용 라인으로
/// 금액과 함께 찍힌다(건대처럼 뭉치지 않음). 따라서 금액 > 0 이면 납부로 본다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudentFeePayment {
    /// 학생회비 금액(원). 미납/미선택이면 0.
    pub council_fee: u64,
    /// 학생회비 납부 여부 (council_fee > 0).
    pub paid: bool,
    /// 납부총액(원). 참고용(디버깅·교차검증).
    pub total_paid: u64,
    /// 이 결과가 실제로 가리키는 학년도(데이터의 PERYR_TXT). 폼이 요청 학기를 무시하고 기본 학기로
    /// 떴는지 호출부가 교차검증하고, UI 가 "N학년도 기준"을 표기하기 위함.
    pub period_year: Option<u32>,
    /// 이 결과가 가리키는 학기 표기(데이터의 PERID_TXT, 예: "1 학기").
    pub period_label: Option<String>,
}

fn get_field(row: &Row, name: &str) -> Option<String> {
    row.iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.to_string_repr())
}

fn find_dataset<'a>(datasets: &'a [DataSet], name: &str) -> Option<&'a Vec<Row>> {
    datasets.iter().find(|(n, _)| n == name).map(|(_, rows)| rows)
}

/// "10,000" / "10000" / "₩10,000" / "10,000원" / "10,000.00" → 10000.
/// 소수부가 있으면 절단(정수 원 단위), 콤마·기호는 제거.
pub(crate) fn parse_amount(s: &str) -> u64 {
    let head = s.split('.').next().unwrap_or(s);
    let digits: String = head.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse().unwrap_or(0)
}

// 2026-10-08 실측(전의찬 계정, OZ fetch)으로 확정:
//   납부확인서 OZ(zcm_get_regist_check_paper_rfc)는 마스터 요약 1행 데이터셋 ET_REGIST_DATA 를 준다.
//   학생회비 = ETC_AMT1 (2017/1 납부자 10,000 / 2026/2 미납 0 으로 교차검증). 납부총액 = TOTAL_AMOUNT.
//   (REG_AMT1=입학금, REG_AMT2=수업료, ETC_AMT4=어학실습비 … ETC_AMT1 이 학생회비 슬롯.)
const DATASET_NAME: &str = "ET_REGIST_DATA";
const FIELD_COUNCIL_FEE: &str = "ETC_AMT1";
const FIELD_TOTAL: &str = "TOTAL_AMOUNT";
// 결과가 가리키는 학기(교차검증·UI 표기용). PERYR_TXT="2017", PERID_TXT="1 학기".
const FIELD_PERYR: &str = "PERYR_TXT";
const FIELD_PERID: &str = "PERID_TXT";

impl StudentFeePayment {
    /// OZ DataModule 의 데이터셋에서 학생회비 납부 정보를 생성합니다.
    ///
    /// 현재는 '마스터 요약 1행(입학금·수업료·…·학생회비·납부총액 컬럼)' 구조를 가정합니다.
    /// recon 으로 실제 구조가 확정되면 DATASET/FIELD 상수(와 필요시 합산 로직)를 교체합니다.
    pub fn from_datasets(datasets: &[DataSet]) -> Result<Self, RusaintError> {
        // recon 미완료 가드 — 상수가 placeholder면 '미납(paid=false)'으로 오인되지 않도록 명시적 에러.
        if DATASET_NAME.starts_with("__RECON_TODO") {
            return Err(ApplicationError::OzDataFetchError(
                "학생회비 데이터셋 매핑이 아직 recon으로 확정되지 않았습니다".to_string(),
            )
            .into());
        }
        // '조회 실패'(데이터셋/행/필드 없음)와 '진짜 미납(금액 0)'을 구분한다.
        // 데이터셋·필드를 못 찾으면 Err(조회 실패) — 조용히 paid:false 로 내리지 않는다.
        let rows = find_dataset(datasets, DATASET_NAME).ok_or_else(|| {
            ApplicationError::OzDataFetchError(format!("데이터셋 '{DATASET_NAME}'을 찾지 못했습니다"))
        })?;
        let row = rows.first().ok_or_else(|| {
            ApplicationError::OzDataFetchError("납부내역 행이 비어 있습니다".to_string())
        })?;
        let council_fee = get_field(row, FIELD_COUNCIL_FEE)
            .map(|s| parse_amount(&s))
            .ok_or_else(|| {
                ApplicationError::OzDataFetchError(format!(
                    "학생회비 필드 '{FIELD_COUNCIL_FEE}'를 찾지 못했습니다"
                ))
            })?;
        // 납부총액은 참고용이라 없어도 0 으로 둔다(판정에 안 쓰임).
        let total_paid = get_field(row, FIELD_TOTAL)
            .map(|s| parse_amount(&s))
            .unwrap_or(0);
        // 결과가 가리키는 학기(교차검증·UI 표기용). 없어도 판정엔 영향 없으니 Option.
        let period_year = get_field(row, FIELD_PERYR).and_then(|s| {
            let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
            digits.parse().ok()
        });
        let period_label = get_field(row, FIELD_PERID).map(|s| s.trim().to_string());
        Ok(Self {
            council_fee,
            paid: council_fee > 0,
            total_paid,
            period_year,
            period_label,
        })
    }

    /// 응답이 실제 가리키는 학기(PERYR/PERID)가 요청과 어긋나면 사유를 돌려준다(일치=None).
    /// 기간 선택이 조용히 실패해 폼이 기본(현재) 학기에 머문 경우를 잡아, 잘못된 paid 를 막는다.
    /// PERYR/PERID 가 응답에 없으면(Option None) 그 축은 검증을 건너뛴다.
    pub(crate) fn term_mismatch(&self, year: u32, semester: SemesterType) -> Option<String> {
        if let Some(py) = self.period_year
            && py != year
        {
            return Some(format!("요청 {year}, 응답 {py}"));
        }
        if let Some(label) = &self.period_label {
            let marker = match semester {
                SemesterType::One => "1",
                SemesterType::Summer => "여름",
                SemesterType::Two => "2",
                SemesterType::Winter => "겨울",
            };
            if !label.contains(marker) {
                return Some(format!("요청 학기 {semester:?}, 응답 '{label}'"));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{StudentFeePayment, parse_amount};
    use crate::model::SemesterType;
    use ozra::types::{DataSet, FieldValue};

    fn payment(period_year: Option<u32>, period_label: Option<&str>) -> StudentFeePayment {
        StudentFeePayment {
            council_fee: 0,
            paid: false,
            total_paid: 0,
            period_year,
            period_label: period_label.map(|s| s.to_string()),
        }
    }

    #[test]
    fn term_mismatch_catches_wrong_year_and_semester() {
        // 요청 == 응답 → None(통과).
        assert!(payment(Some(2026), Some("2 학기")).term_mismatch(2026, SemesterType::Two).is_none());
        assert!(payment(Some(2017), Some("1 학기")).term_mismatch(2017, SemesterType::One).is_none());
        // 연도 불일치(기간 선택 실패로 기본 학기가 온 경우) → Some.
        assert!(payment(Some(2026), Some("2 학기")).term_mismatch(2017, SemesterType::One).is_some());
        // 같은 연도, 학기만 불일치 → Some.
        assert!(payment(Some(2026), Some("2 학기")).term_mismatch(2026, SemesterType::One).is_some());
        // 여름·겨울 마커.
        assert!(payment(Some(2026), Some("여름학기")).term_mismatch(2026, SemesterType::Summer).is_none());
        assert!(payment(Some(2026), Some("겨울학기")).term_mismatch(2026, SemesterType::Winter).is_none());
        assert!(payment(Some(2026), Some("여름학기")).term_mismatch(2026, SemesterType::One).is_some());
        // PERYR/PERID 가 응답에 없으면 그 축은 검증 생략 → 통과(조용한 오판보다 가용성 우선).
        assert!(payment(None, None).term_mismatch(2017, SemesterType::One).is_none());
    }

    fn regist_row(council: &str, total: &str, peryr: &str, perid: &str) -> Vec<(String, FieldValue)> {
        vec![
            ("PERYR_TXT".to_string(), FieldValue::String(peryr.to_string())),
            ("PERID_TXT".to_string(), FieldValue::String(perid.to_string())),
            ("REG_AMT2".to_string(), FieldValue::String("4411000".to_string())),
            ("ETC_AMT1".to_string(), FieldValue::String(council.to_string())),
            ("ETC_AMT4".to_string(), FieldValue::String("140000".to_string())),
            ("TOTAL_AMOUNT".to_string(), FieldValue::String(total.to_string())),
        ]
    }

    #[test]
    fn from_datasets_payer() {
        // 2017/1 실측: ETC_AMT1=10000(학생회비), TOTAL_AMOUNT=5461000, PERYR_TXT=2017, PERID_TXT="1 학기".
        let datasets: Vec<DataSet> = vec![(
            "ET_REGIST_DATA".to_string(),
            vec![regist_row("10000", "5461000", "2017", "1 학기")],
        )];
        let r = StudentFeePayment::from_datasets(&datasets).unwrap();
        assert_eq!(r.council_fee, 10_000);
        assert!(r.paid);
        assert_eq!(r.total_paid, 5_461_000);
        assert_eq!(r.period_year, Some(2017));
        assert_eq!(r.period_label.as_deref(), Some("1 학기"));
    }

    #[test]
    fn from_datasets_non_payer() {
        // 2026/2 실측: ETC_AMT1=0(미납).
        let datasets: Vec<DataSet> = vec![(
            "ET_REGIST_DATA".to_string(),
            vec![regist_row("0", "4765000", "2026", "2 학기")],
        )];
        let r = StudentFeePayment::from_datasets(&datasets).unwrap();
        assert_eq!(r.council_fee, 0);
        assert!(!r.paid);
        assert_eq!(r.period_year, Some(2026));
        assert_eq!(r.period_label.as_deref(), Some("2 학기"));
    }

    #[test]
    fn from_datasets_missing_is_err() {
        // 데이터셋/행/필드가 없으면 Err — '조회 실패'를 조용히 '미납'으로 내리지 않는다.
        let empty: Vec<DataSet> = vec![];
        assert!(StudentFeePayment::from_datasets(&empty).is_err());
        let no_field: Vec<DataSet> =
            vec![("ET_REGIST_DATA".to_string(), vec![vec![]])];
        assert!(StudentFeePayment::from_datasets(&no_field).is_err());
    }


    #[test]
    fn parse_amount_variants() {
        assert_eq!(parse_amount("10,000"), 10_000);
        assert_eq!(parse_amount("10000"), 10_000);
        assert_eq!(parse_amount("₩10,000"), 10_000);
        assert_eq!(parse_amount("10,000원"), 10_000);
        assert_eq!(parse_amount("0"), 0);
        assert_eq!(parse_amount(""), 0);
        assert_eq!(parse_amount("5,461,000"), 5_461_000);
        // 소수부는 절단(정수 원 단위) — 100배 오독 방지.
        assert_eq!(parse_amount("10,000.00"), 10_000);
        assert_eq!(parse_amount("10000.50"), 10_000);
    }
}
