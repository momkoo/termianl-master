/// 한글 자모 조합 모듈
/// 개별 자모(ㄱ, ㅏ, ㄴ 등)를 받아서 완성된 한글 음절(간 등)로 조합

use std::char;

/// 한글 자모 범위
const JAMO_L_BASE: u32 = 0x1100; // 초성 시작
const JAMO_V_BASE: u32 = 0x1161; // 중성 시작
const JAMO_T_BASE: u32 = 0x11A7; // 종성 시작

/// 완성된 한글 음절 범위
const SYLLABLE_BASE: u32 = 0xAC00; // 가
const L_COUNT: u32 = 19;  // 초성 개수
const V_COUNT: u32 = 21;  // 중성 개수
const T_COUNT: u32 = 28;  // 종성 개수 (빈 종성 포함)
const N_COUNT: u32 = V_COUNT * T_COUNT; // 588

/// 초성 자모 매핑
const LEADING_CONSONANTS: &[(char, u32)] = &[
    ('ㄱ', 0), ('ㄲ', 1), ('ㄴ', 2), ('ㄷ', 3), ('ㄸ', 4),
    ('ㄹ', 5), ('ㅁ', 6), ('ㅂ', 7), ('ㅃ', 8), ('ㅅ', 9),
    ('ㅆ', 10), ('ㅇ', 11), ('ㅈ', 12), ('ㅉ', 13), ('ㅊ', 14),
    ('ㅋ', 15), ('ㅌ', 16), ('ㅍ', 17), ('ㅎ', 18),
];

/// 중성 자모 매핑
const VOWELS: &[(char, u32)] = &[
    ('ㅏ', 0), ('ㅐ', 1), ('ㅑ', 2), ('ㅒ', 3), ('ㅓ', 4),
    ('ㅔ', 5), ('ㅕ', 6), ('ㅖ', 7), ('ㅗ', 8), ('ㅘ', 9),
    ('ㅙ', 10), ('ㅚ', 11), ('ㅛ', 12), ('ㅜ', 13), ('ㅝ', 14),
    ('ㅞ', 15), ('ㅟ', 16), ('ㅠ', 17), ('ㅡ', 18), ('ㅢ', 19),
    ('ㅣ', 20),
];

/// 종성 자모 매핑
const TRAILING_CONSONANTS: &[(char, u32)] = &[
    ('\0', 0), // 빈 종성
    ('ㄱ', 1), ('ㄲ', 2), ('ㄳ', 3), ('ㄴ', 4), ('ㄵ', 5),
    ('ㄶ', 6), ('ㄷ', 7), ('ㄹ', 8), ('ㄺ', 9), ('ㄻ', 10),
    ('ㄼ', 11), ('ㄽ', 12), ('ㄾ', 13), ('ㄿ', 14), ('ㅀ', 15),
    ('ㅁ', 16), ('ㅂ', 17), ('ㅄ', 18), ('ㅅ', 19), ('ㅆ', 20),
    ('ㅇ', 21), ('ㅈ', 22), ('ㅊ', 23), ('ㅋ', 24), ('ㅌ', 25),
    ('ㅍ', 26), ('ㅎ', 27),
];

/// 한글 조합 상태
#[derive(Debug, Clone)]
pub struct HangulComposer {
    leading: Option<u32>,   // 초성
    vowel: Option<u32>,     // 중성
    trailing: Option<u32>,  // 종성
}

impl Default for HangulComposer {
    fn default() -> Self {
        Self::new()
    }
}

impl HangulComposer {
    pub fn new() -> Self {
        Self {
            leading: None,
            vowel: None,
            trailing: None,
        }
    }

    /// 자모가 초성인지 확인
    fn is_leading_consonant(&self, ch: char) -> Option<u32> {
        LEADING_CONSONANTS.iter()
            .find(|(c, _)| *c == ch)
            .map(|(_, idx)| *idx)
    }

    /// 자모가 중성인지 확인
    fn is_vowel(&self, ch: char) -> Option<u32> {
        VOWELS.iter()
            .find(|(c, _)| *c == ch)
            .map(|(_, idx)| *idx)
    }

    /// 자모가 종성인지 확인
    fn is_trailing_consonant(&self, ch: char) -> Option<u32> {
        TRAILING_CONSONANTS.iter()
            .skip(1) // 빈 종성 제외
            .find(|(c, _)| *c == ch)
            .map(|(_, idx)| *idx)
    }

    /// 현재 상태에서 완성된 음절 생성
    pub fn get_current_syllable(&self) -> Option<char> {
        if let (Some(l), Some(v)) = (self.leading, self.vowel) {
            let t = self.trailing.unwrap_or(0);
            let syllable_code = SYLLABLE_BASE + (l * N_COUNT) + (v * T_COUNT) + t;
            char::from_u32(syllable_code)
        } else {
            None
        }
    }

    /// 자모 입력 처리
    pub fn input_jamo(&mut self, ch: char) -> CompositionResult {
        // 초성 처리
        if let Some(l_idx) = self.is_leading_consonant(ch) {
            if self.leading.is_none() {
                // 첫 초성
                self.leading = Some(l_idx);
                return CompositionResult::Composing;
            } else if self.vowel.is_none() {
                // 초성만 있는 상태에서 새 초성 -> 기존 초성 출력 후 새 초성 시작
                let prev = self.get_current_syllable();
                self.clear();
                self.leading = Some(l_idx);
                return CompositionResult::CompletedWithNew(prev, None);
            } else {
                // 초성+중성 있는 상태에서 새 초성 -> 기존 음절 완성 후 새 초성 시작
                let completed = self.get_current_syllable();
                self.clear();
                self.leading = Some(l_idx);
                return CompositionResult::CompletedWithNew(completed, None);
            }
        }

        // 중성 처리
        if let Some(v_idx) = self.is_vowel(ch) {
            if self.leading.is_some() && self.vowel.is_none() {
                // 초성 다음 중성
                self.vowel = Some(v_idx);
                return CompositionResult::Composing;
            } else if self.leading.is_some() && self.vowel.is_some() {
                // 이미 중성이 있으면 현재 음절 완성 후 새로 시작 (일단 단순 처리)
                let completed = self.get_current_syllable();
                self.clear();
                return CompositionResult::CompletedWithNew(completed, Some(ch));
            } else {
                // 초성 없이 중성만 -> 그냥 출력
                return CompositionResult::DirectOutput(ch);
            }
        }

        // 종성 처리
        if let Some(t_idx) = self.is_trailing_consonant(ch) {
            if self.leading.is_some() && self.vowel.is_some() && self.trailing.is_none() {
                // 초성+중성 다음 종성
                self.trailing = Some(t_idx);
                return CompositionResult::Composing;
            } else {
                // 다른 경우는 복잡하므로 일단 직접 출력
                return CompositionResult::DirectOutput(ch);
            }
        }

        // 한글 자모가 아닌 문자는 직접 출력
        CompositionResult::DirectOutput(ch)
    }

    /// 현재 조합 상태 클리어
    pub fn clear(&mut self) {
        self.leading = None;
        self.vowel = None;
        self.trailing = None;
    }

    /// 현재 조합 중인지 확인
    pub fn is_composing(&self) -> bool {
        self.leading.is_some() || self.vowel.is_some() || self.trailing.is_some()
    }

    /// 강제로 현재 음절 완성
    pub fn flush(&mut self) -> Option<char> {
        let result = self.get_current_syllable();
        self.clear();
        result
    }
}

/// 조합 결과
#[derive(Debug)]
pub enum CompositionResult {
    /// 조합 중 (아직 출력하지 않음)
    Composing,
    /// 직접 출력 (조합되지 않는 문자)
    DirectOutput(char),
    /// 완성된 음절과 함께 새로운 조합 시작
    CompletedWithNew(Option<char>, Option<char>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hangul_composition() {
        let mut composer = HangulComposer::new();

        // "안" 조합 테스트
        match composer.input_jamo('ㅇ') {
            CompositionResult::Composing => (),
            _ => panic!("Expected composing"),
        }

        match composer.input_jamo('ㅏ') {
            CompositionResult::Composing => (),
            _ => panic!("Expected composing"),
        }

        match composer.input_jamo('ㄴ') {
            CompositionResult::Composing => (),
            _ => panic!("Expected composing"),
        }

        let result = composer.get_current_syllable();
        assert_eq!(result, Some('안'));
    }
}