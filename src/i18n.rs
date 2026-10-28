//! Text shown to visitors, in Arabic and English. Arabic is the default; a cookie remembers
//! the visitor's choice.

use topcoat::context::Cx;
use topcoat::cookie::{Cookies, cookies};

use crate::queue::Rejection;
use crate::tools::diacritization::{MAX_TEXT_CHARACTERS, Model, Problem};
use crate::tools::ocr::{DocumentProblem, MAX_PAGES};
use crate::tools::transcription::{AudioProblem, MAX_DURATION};
use crate::tools::{KEEP_FINISHED, MAX_UPLOAD_BYTES};

/// The cookie holding the visitor's language.
pub const COOKIE: &str = "lang";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    Ar,
    En,
}

impl Locale {
    pub const ALL: [Self; 2] = [Self::Ar, Self::En];

    /// The visitor's language: the one they chose, or Arabic.
    pub fn of(cx: &Cx) -> Self {
        cookies(cx)
            .get(COOKIE)
            .and_then(|cookie| Self::from_code(cookie.value()))
            .unwrap_or(Self::Ar)
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|locale| locale.code() == code)
    }

    /// The language's code, for `lang` attributes and the cookie.
    pub fn code(self) -> &'static str {
        match self {
            Self::Ar => "ar",
            Self::En => "en",
        }
    }

    /// The direction of text in this language, for `dir` attributes.
    pub fn dir(self) -> &'static str {
        match self {
            Self::Ar => "rtl",
            Self::En => "ltr",
        }
    }

    pub fn text(self) -> &'static Text {
        match self {
            Self::Ar => &AR,
            Self::En => &EN,
        }
    }

    /// The locale of link previews, in Open Graph's form.
    pub fn og_locale(self) -> &'static str {
        match self {
            Self::Ar => "ar_AR",
            Self::En => "en_US",
        }
    }

    /// A page's `<title>`: its own name, then the site's.
    pub fn page_title(self, page: &str) -> String {
        format!("{page} — {}", self.text().site_name)
    }

    /// The transcription tool's description for search results and link previews.
    pub fn transcription_summary(self) -> String {
        match self {
            Self::Ar => format!(
                "فرّغ تسجيلاتك الصوتية بصيغة MP3 أو M4A إلى نص مكتوب وملف ترجمة SRT، مجانًا ودون حساب. حتى {} ميغابايت و{} ساعات.",
                max_megabytes(),
                max_hours()
            ),
            Self::En => format!(
                "Transcribe MP3 and M4A recordings into text and SRT subtitles, free and with no account. Up to {} MB and {} hours.",
                max_megabytes(),
                max_hours()
            ),
        }
    }

    pub fn rejection(self, rejection: Rejection) -> &'static str {
        let text = &self.text().transcription;
        match rejection {
            Rejection::Closed => text.closed,
            Rejection::Full => text.full,
            Rejection::ActiveJob => text.active_job,
            Rejection::NoReservation => text.no_reservation,
        }
    }

    pub fn audio_problem(self, problem: AudioProblem) -> String {
        let text = &self.text().transcription;
        match problem {
            AudioProblem::Missing => text.missing.to_owned(),
            AudioProblem::WrongType => text.wrong_type.to_owned(),
            AudioProblem::Unreadable => text.unreadable.to_owned(),
            AudioProblem::TooLarge => self.audio_too_large(),
            AudioProblem::TooLong => match self {
                Self::Ar => format!("مدة التسجيل أطول من {} ساعات.", max_hours()),
                Self::En => format!("The recording is longer than {} hours.", max_hours()),
            },
        }
    }

    /// The message for a file over the size limit; also used by the upload page's own check.
    pub fn too_large(self) -> String {
        match self {
            Self::Ar => format!("حجم الملف أكبر من {} ميغابايت.", max_megabytes()),
            Self::En => format!("The file is larger than {} MB.", max_megabytes()),
        }
    }

    pub fn upload_limits(self) -> String {
        match self {
            Self::Ar => format!(
                "ملف MP3 أو M4A، حتى {} ميغابايت و{} ساعات.",
                max_megabytes(),
                max_hours()
            ),
            Self::En => format!(
                "MP3 or M4A, up to {} MB and {} hours.",
                max_megabytes(),
                max_hours()
            ),
        }
    }

    /// The size-limit message for a recording, with how to make one fit.
    pub fn audio_too_large(self) -> String {
        match self {
            Self::Ar => format!(
                "{} صدّره بصيغة MP3 بمعدل 64 كيلوبت في الثانية وبقناة واحدة (Mono)، فيبقى الكلام واضحًا ويتسع الحد لتسجيل يصل إلى {} ساعات.",
                self.too_large(),
                max_hours()
            ),
            Self::En => format!(
                "{} Re-export it as MP3 at 64 kbps, mono: speech stays clear, and up to {} hours fits.",
                self.too_large(),
                max_hours()
            ),
        }
    }

    pub fn privacy_note(self) -> String {
        match self {
            Self::Ar => format!(
                "نحذف الملف الصوتي فور انتهاء التفريغ، ونحذف النص بعد {} دقيقة.",
                keep_minutes()
            ),
            Self::En => format!(
                "We delete your audio as soon as the transcription ends, and the transcript after {} minutes.",
                keep_minutes()
            ),
        }
    }

    pub fn waiting(self, position: usize) -> String {
        match self {
            Self::Ar => format!("ملفك في الطابور، وترتيبك {position}."),
            Self::En => format!("Your file is in the queue, at position {position}."),
        }
    }

    pub fn percent(self, percent: u8) -> String {
        match self {
            Self::Ar => format!("{percent}٪"),
            Self::En => format!("{percent}%"),
        }
    }

    pub fn ocr_rejection(self, rejection: Rejection) -> &'static str {
        let text = &self.text().ocr;
        match rejection {
            Rejection::Closed => text.closed,
            Rejection::Full => text.full,
            Rejection::ActiveJob => text.active_job,
            Rejection::NoReservation => text.no_reservation,
        }
    }

    pub fn document_problem(self, problem: DocumentProblem) -> String {
        let text = &self.text().ocr;
        match problem {
            DocumentProblem::Missing => text.missing.into(),
            DocumentProblem::WrongType => text.wrong_type.into(),
            DocumentProblem::Unreadable => text.unreadable.into(),
            DocumentProblem::TooLarge => self.too_large(),
            DocumentProblem::TooManyPages => match self {
                Self::Ar => format!("الملف يتجاوز الحد الأقصى البالغ {MAX_PAGES} صفحة."),
                Self::En => format!("The file exceeds the {MAX_PAGES}-page limit."),
            },
        }
    }

    pub fn ocr_limits(self) -> String {
        match self {
            Self::Ar => format!(
                "ملف PDF أو JPG أو PNG واحد، حتى {} ميغابايت و{MAX_PAGES} صفحة. نتعرّف على اللغة تلقائيًا.",
                max_megabytes()
            ),
            Self::En => format!(
                "One PDF, JPG or PNG, up to {} MB and {MAX_PAGES} pages. The language is detected automatically.",
                max_megabytes()
            ),
        }
    }

    pub fn pages_done(self, done: usize, total: usize) -> String {
        match self {
            Self::Ar => format!("اكتملت {done} من {total} صفحة"),
            Self::En => format!("{done} of {total} pages complete"),
        }
    }

    pub fn page_number(self, number: usize) -> String {
        match self {
            Self::Ar => format!("الصفحة {number}"),
            Self::En => format!("Page {number}"),
        }
    }

    pub fn kept_for(self) -> String {
        match self {
            Self::Ar => format!("يبقى النص متاحًا لمدة {} دقيقة.", keep_minutes()),
            Self::En => format!(
                "The transcript stays available for {} minutes.",
                keep_minutes()
            ),
        }
    }

    pub fn diacritization_rejection(self, rejection: Rejection) -> &'static str {
        let text = &self.text().diacritization;
        match rejection {
            Rejection::Closed => text.closed,
            Rejection::Full => text.full,
            Rejection::ActiveJob => text.active_job,
            Rejection::NoReservation => text.no_reservation,
        }
    }

    pub fn diacritization_problem(self, problem: Problem) -> String {
        let text = &self.text().diacritization;
        match problem {
            Problem::Missing => text.missing.into(),
            Problem::WrongType => text.wrong_type.into(),
            Problem::TooLarge => self.too_large(),
            Problem::InvalidUtf8 => text.invalid_utf8.into(),
            Problem::InvalidDocx => text.invalid_docx.into(),
            Problem::UnsupportedDocx => text.unsupported_docx.into(),
            Problem::DocxLimits => text.docx_limits.into(),
            Problem::TextTooLong => self.diacritization_text_limit(),
            Problem::Unreadable => text.unreadable.into(),
        }
    }

    pub fn diacritization_model(self, model: Model) -> &'static str {
        let text = &self.text().diacritization;
        match model {
            Model::Shakkelha => text.shakkelha,
            Model::Catt => text.catt,
        }
    }

    pub fn diacritization_text_limit(self) -> String {
        match self {
            Self::Ar => {
                format!("أدخل نصًا لا يتجاوز {MAX_TEXT_CHARACTERS} حرفًا، بما فيها الحركات والمسافات.")
            }
            Self::En => format!(
                "Enter up to {MAX_TEXT_CHARACTERS} characters, including diacritics and spaces."
            ),
        }
    }

    pub fn diacritization_count(self, count: usize) -> String {
        self.text()
            .diacritization
            .character_count
            .replace("{count}", &count.to_string())
            .replace("{max}", &MAX_TEXT_CHARACTERS.to_string())
    }

    pub fn diacritization_limits(self) -> String {
        match self {
            Self::Ar => format!(
                "ملف TXT بترميز UTF-8 أو DOCX واحد، حتى {} ميغابايت. يحتفظ الملف المشكول بصيغته الأصلية. ملفات DOC غير مدعومة، وقد تحتاج مستندات Word الكبيرة أو المعقدة إلى التقسيم.",
                max_megabytes()
            ),
            Self::En => format!(
                "One UTF-8 TXT or DOCX file, up to {} MB. The diacritized file keeps its format. DOC files are not supported, and large or complex Word documents may need to be split.",
                max_megabytes()
            ),
        }
    }

    pub fn diacritization_waiting(self, position: usize) -> String {
        match self {
            Self::Ar => format!("طلبك في الطابور، وترتيبك {position}."),
            Self::En => format!("Your request is in the queue, at position {position}."),
        }
    }

    pub fn diacritization_privacy(self) -> String {
        match self {
            Self::Ar => format!(
                "تُعالَج النصوص والملفات على خادمنا. نحذف الأصل والنتيجة بعد {} دقيقة من انتهاء المهمة.",
                keep_minutes()
            ),
            Self::En => format!(
                "Text and documents are processed on our server. We delete the original and result {} minutes after the job ends.",
                keep_minutes()
            ),
        }
    }

    pub fn diacritization_kept_for(self) -> String {
        match self {
            Self::Ar => format!("يبقى الأصل والنتيجة متاحين لمدة {} دقيقة.", keep_minutes()),
            Self::En => format!(
                "The original and result stay available for {} minutes.",
                keep_minutes()
            ),
        }
    }
}

fn max_megabytes() -> u64 {
    MAX_UPLOAD_BYTES / 1_000_000
}

fn max_hours() -> u64 {
    MAX_DURATION.as_secs() / 3600
}

fn keep_minutes() -> u64 {
    KEEP_FINISHED.as_secs() / 60
}

/// Fixed text of the whole site.
pub struct Text {
    /// This language's name, written in it, for the language switch.
    pub name: &'static str,
    /// The site's name in this language.
    pub site_name: &'static str,
    /// The logo link's accessible name.
    pub home_label: &'static str,
    pub skip_to_main: &'static str,
    pub tagline: &'static str,
    pub lead: &'static str,
    /// The site's description for search results and link previews.
    pub description: &'static str,
    /// What the link-preview image shows.
    pub preview_alt: &'static str,
    pub free: &'static str,
    pub files_deleted: &'static str,
    pub arabic_first: &'static str,
    pub tools_title: &'static str,
    pub open_tool: &'static str,
    pub more_tools: &'static str,
    pub source_code: &'static str,
    /// The footer's mention of the initiative, around its linked name.
    pub initiative_before: &'static str,
    pub initiative_name: &'static str,
    pub initiative_after: &'static str,
    pub social: SocialText,
    pub not_found_title: &'static str,
    pub not_found: &'static str,
    pub back_home: &'static str,
    pub transcription: TranscriptionText,
    pub ocr: OcrText,
    pub diacritization: DiacritizationText,
    pub cancelled_user: &'static str,
    pub review: ReviewText,
    pub workspace: WorkspaceText,
}

/// Search and transcript review controls shared by result viewers.
pub struct ReviewText {
    pub search: &'static str,
    pub ignore_marks: &'static str,
    pub ignore_tatweel: &'static str,
    pub match_count: &'static str,
    pub no_matches: &'static str,
    pub previous_match: &'static str,
    pub next_match: &'static str,
    pub clear_search: &'static str,
    pub searching: &'static str,
    pub search_unavailable: &'static str,
    pub copy_all: &'static str,
    /// Opens the menu of a result's downloads, or names its only download.
    pub download: &'static str,
    pub copy_cue: &'static str,
    pub copied: &'static str,
    pub copy_failed: &'static str,
    pub copy_label: &'static str,
    pub copy_close: &'static str,
    pub show_timestamps: &'static str,
    pub go_to_cue: &'static str,
    pub cue_at: &'static str,
}

/// Persistent workspace controls and lifecycle messages.
pub struct WorkspaceText {
    pub reserving: &'static str,
    pub validating: &'static str,
    pub observing: &'static str,
    pub delayed: &'static str,
    pub retry_observation: &'static str,
    pub recovery: &'static str,
    pub view_result: &'static str,
    pub previous_result: &'static str,
    pub previous_model: &'static str,
    pub replace_file: &'static str,
    pub remove_file: &'static str,
    pub bytes: &'static str,
    pub kilobytes: &'static str,
    pub megabytes: &'static str,
    pub language_reload: &'static str,
    pub source_needed: &'static str,
    pub missing: &'static str,
    pub result: &'static str,
}

/// The initiative's accounts, linked from the footer.
pub struct SocialText {
    /// The list's accessible name.
    pub label: &'static str,
    pub telegram: Account,
    pub youtube: Account,
    pub x: Account,
    pub facebook: Account,
    pub github: Account,
    pub email: Account,
}

/// One of the initiative's accounts: the link's short text, and its full accessible name.
pub struct Account {
    pub name: &'static str,
    pub label: &'static str,
}

/// Fixed text of the transcription tool.
pub struct TranscriptionText {
    pub input_formats: &'static str,
    pub output_formats: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub all_tools: &'static str,
    pub transcript_title: &'static str,
    pub file_label: &'static str,
    pub choose_file: &'static str,
    pub submit: &'static str,
    pub uploading: &'static str,
    pub upload_failed: &'static str,
    pub needs_javascript: &'static str,
    pub keep_open: &'static str,
    pub reserved: &'static str,
    pub running: &'static str,
    pub done: &'static str,
    pub download_text: &'static str,
    pub download_srt: &'static str,
    pub no_speech: &'static str,
    pub failed: &'static str,
    pub cancelled_unwatched: &'static str,
    pub cancelled_shutdown: &'static str,
    pub another: &'static str,
    pub closed: &'static str,
    pub full: &'static str,
    pub active_job: &'static str,
    pub back_to_job: &'static str,
    pub no_reservation: &'static str,
    pub missing: &'static str,
    pub wrong_type: &'static str,
    pub unreadable: &'static str,
}

/// Fixed text of the OCR tool.
pub struct OcrText {
    pub review: OcrReviewText,
    pub name: &'static str,
    pub description: &'static str,
    pub input_formats: &'static str,
    pub output_formats: &'static str,
    pub all_tools: &'static str,
    pub preview_title: &'static str,
    pub file_label: &'static str,
    pub choose_file: &'static str,
    pub submit: &'static str,
    pub uploading: &'static str,
    pub upload_failed: &'static str,
    pub needs_javascript: &'static str,
    pub keep_open: &'static str,
    pub reserved: &'static str,
    pub running: &'static str,
    pub done: &'static str,
    pub download_text: &'static str,
    pub download_docx: &'static str,
    pub download_json: &'static str,
    pub no_text: &'static str,
    pub failed: &'static str,
    pub cancelled_unwatched: &'static str,
    pub cancelled_shutdown: &'static str,
    pub another: &'static str,
    pub closed: &'static str,
    pub full: &'static str,
    pub active_job: &'static str,
    pub back_to_job: &'static str,
    pub no_reservation: &'static str,
    pub missing: &'static str,
    pub wrong_type: &'static str,
    pub unreadable: &'static str,
    pub unavailable: &'static str,
    pub privacy: &'static str,
    pub kept_for: &'static str,
}

/// OCR review controls and local preview messages.
#[derive(serde::Serialize)]
pub struct OcrReviewText {
    pub source_title: &'static str,
    pub text_title: &'static str,
    pub view_label: &'static str,
    pub source_view: &'static str,
    pub text_view: &'static str,
    pub previous_page: &'static str,
    pub next_page: &'static str,
    pub page_label: &'static str,
    pub page_position: &'static str,
    pub page_name: &'static str,
    pub page_error: &'static str,
    pub fit_width: &'static str,
    pub fit_page: &'static str,
    pub zoom_in: &'static str,
    pub zoom_out: &'static str,
    pub zoom_label: &'static str,
    pub copy_page: &'static str,
    pub source_missing: &'static str,
    pub source_confirm: &'static str,
    pub source_file: &'static str,
    pub source_attach: &'static str,
    pub source_local: &'static str,
    pub source_browser: &'static str,
    pub source_loading: &'static str,
    pub source_failed: &'static str,
    pub source_mismatch: &'static str,
    pub source_required: &'static str,
    pub all_blank: &'static str,
}

/// Fixed text of the Arabic diacritization tool, including messages used by its script.
pub struct DiacritizationText {
    pub name: &'static str,
    pub description: &'static str,
    pub input_formats: &'static str,
    pub output_formats: &'static str,
    pub all_tools: &'static str,
    pub replaces_marks: &'static str,
    pub input_mode: &'static str,
    pub text_mode: &'static str,
    pub document_mode: &'static str,
    pub model: &'static str,
    pub shakkelha: &'static str,
    pub catt: &'static str,
    pub choose_model: &'static str,
    pub model_unavailable: &'static str,
    pub source_label: &'static str,
    pub placeholder: &'static str,
    pub character_count: &'static str,
    pub text_missing: &'static str,
    pub result_label: &'static str,
    pub submit_text: &'static str,
    pub submit_document: &'static str,
    pub file_label: &'static str,
    pub choose_file: &'static str,
    pub uploading: &'static str,
    pub submitting: &'static str,
    pub upload_failed: &'static str,
    pub needs_javascript: &'static str,
    pub keep_open: &'static str,
    pub reserved: &'static str,
    pub running: &'static str,
    pub done: &'static str,
    pub failed: &'static str,
    pub cancelled_unwatched: &'static str,
    pub cancelled_shutdown: &'static str,
    pub cancel: &'static str,
    pub cancelling: &'static str,
    pub cancel_failed: &'static str,
    pub download_text: &'static str,
    pub download_docx: &'static str,
    pub download_original: &'static str,
    pub another: &'static str,
    pub result_placeholder: &'static str,
    pub closed: &'static str,
    pub full: &'static str,
    pub active_job: &'static str,
    pub back_to_job: &'static str,
    pub no_reservation: &'static str,
    pub missing: &'static str,
    pub wrong_type: &'static str,
    pub invalid_utf8: &'static str,
    pub invalid_docx: &'static str,
    pub unsupported_docx: &'static str,
    pub docx_limits: &'static str,
    pub unreadable: &'static str,
    pub unavailable: &'static str,
}

// Preserve the requested Arabic model spelling and its combining-mark order.
#[allow(clippy::unicode_not_nfc)]
static AR: Text = Text {
    name: "العربية",
    site_name: "أدوات العربية",
    home_label: "الصفحة الرئيسية",
    skip_to_main: "انتقل إلى المحتوى الرئيسي",
    tagline: "أدوات مجانية للغة العربية",
    lead: "أدوات بسيطة لمن يعمل بالعربية: ارفع ملفك واحصل على النتيجة، دون حساب أو تثبيت.",
    description: "أدوات مجانية لتفريغ الصوت واستخراج النص من ملفات PDF والصور وتشكيل النص العربي، دون حساب أو تثبيت.",
    preview_alt: "شعار أدوات العربية، وتحته: أدوات مجانية للغة العربية. تفريغ الصوت · استخراج النص من PDF والصور · التشكيل التلقائي",
    free: "مجانية بلا حساب",
    files_deleted: "تُحذف ملفاتك تلقائيًا",
    arabic_first: "للعربية أولًا",
    tools_title: "الأدوات",
    open_tool: "افتح الأداة",
    more_tools: "المزيد من الأدوات قريبًا.",
    source_code: "الشيفرة المصدرية",
    initiative_before: " مشروع من مبادرة ",
    initiative_name: "الكتب الميسّرة",
    initiative_after: ".",
    social: SocialText {
        label: "روابط التواصل",
        telegram: Account {
            name: "تيليجرام",
            label: "قناتنا على تيليجرام",
        },
        youtube: Account {
            name: "يوتيوب",
            label: "قناتنا على يوتيوب",
        },
        x: Account {
            name: "إكس",
            label: "حسابنا على إكس",
        },
        facebook: Account {
            name: "فيسبوك",
            label: "صفحتنا على فيسبوك",
        },
        github: Account {
            name: "GitHub",
            label: "حسابنا على GitHub للمبرمجين",
        },
        email: Account {
            name: "البريد الإلكتروني",
            label: "بريدنا الإلكتروني",
        },
    },
    not_found_title: "الصفحة غير موجودة",
    not_found: "هذه الصفحة غير موجودة، أو انتهت صلاحيتها.",
    back_home: "العودة إلى الرئيسية",
    cancelled_user: "أُلغيت المهمة بطلبك.",
    workspace: WorkspaceText {
        reserving: "جارٍ طلب مكان في الطابور…",
        validating: "اكتمل الرفع. جارٍ التحقق من الملف…",
        observing: "جارٍ انتظار حالة المهمة…",
        delayed: "تأخرت تحديثات الحالة. يمكنك إعادة المحاولة أو فتح هذه المهمة.",
        retry_observation: "إعادة محاولة تحديث الحالة",
        recovery: "فتح هذه المهمة",
        view_result: "عرض النتيجة",
        previous_result: "النتيجة السابقة",
        previous_model: "النتيجة السابقة — {model}",
        replace_file: "استبدال الملف",
        remove_file: "إزالة الملف",
        bytes: "بايت",
        kilobytes: "كيلوبايت",
        megabytes: "ميغابايت",
        language_reload: "يؤدي تغيير اللغة إلى إعادة تحميل الصفحة. قد تفقد التعديلات غير المرسلة والمعاينة المحلية.",
        source_needed: "ألصق النص أو اختر الملف مجددًا لبدء مهمة أخرى.",
        missing: "هذه المهمة غير متاحة أو انتهت صلاحيتها. يمكنك نسخ النص المعروض هنا.",
        result: "النتيجة",
    },
    review: ReviewText {
        search: "ابحث في النص المستخرج",
        ignore_marks: "تجاهل الحركات الشائعة",
        ignore_tatweel: "تجاهل التطويل",
        match_count: "المطابقة {current} من {total}",
        no_matches: "لم يُعثر على مطابقات",
        previous_match: "المطابقة السابقة",
        next_match: "المطابقة التالية",
        clear_search: "مسح البحث",
        searching: "جارٍ البحث…",
        search_unavailable: "يحتاج البحث إلى متصفح أحدث. يمكنك تحديد النص ونسخه وتنزيله.",
        copy_all: "نسخ النص كاملًا",
        download: "تنزيل",
        copy_cue: "نسخ هذا المقطع",
        copied: "نُسخ النص",
        copy_failed: "تعذّر النسخ. حدّد النص وانسخه يدويًا.",
        copy_label: "النص المراد نسخه",
        copy_close: "إغلاق لوحة النسخ",
        show_timestamps: "إظهار الأوقات",
        go_to_cue: "الانتقال إلى مقطع",
        cue_at: "المقطع {number} عند {time}",
    },
    diacritization: DiacritizationText {
        name: "تشكيل النص العربي",
        description: "أضف التشكيل إلى نص عربي أو مستند TXT أو DOCX، باستخدام شكِّلها أو CATT.",
        input_formats: "نص · TXT · DOCX",
        output_formats: "نص مشكول · TXT · DOCX",
        all_tools: "كل الأدوات",
        replaces_marks: "تُستبدل الحركات الشائعة الموجودة في النص بتشكيل جديد. راجع النتيجة قبل استخدامها.",
        input_mode: "طريقة الإدخال",
        text_mode: "نص",
        document_mode: "مستند",
        model: "نموذج التشكيل",
        shakkelha: "شكِّلها",
        catt: "CATT",
        choose_model: "اختر نموذج التشكيل: شكِّلها أو CATT.",
        model_unavailable: "النموذج المختار غير متاح مؤقتًا. جرّب النموذج الآخر أو عُد لاحقًا.",
        source_label: "النص الأصلي",
        placeholder: "اكتب النص العربي أو ألصقه هنا…",
        character_count: "{count} / {max} حرف",
        text_missing: "أدخل نصًا أولًا.",
        result_label: "النص المشكول",
        submit_text: "شكِّل النص",
        submit_document: "شكِّل المستند",
        file_label: "ملف TXT أو DOCX",
        choose_file: "اختر مستندًا، أو اسحبه وأفلته هنا",
        uploading: "جارٍ رفع الملف…",
        submitting: "جارٍ إرسال النص…",
        upload_failed: "تعذّر إرسال الطلب. تحقّق من اتصالك وحاول مرة أخرى.",
        needs_javascript: "تحتاج هذه الصفحة إلى JavaScript.",
        keep_open: "أبقِ هذه الصفحة مفتوحة حتى ينتهي التشكيل، فإغلاقها يلغي المهمة.",
        reserved: "ننتظر وصول النص أو المستند…",
        running: "جارٍ التشكيل…",
        done: "اكتمل التشكيل.",
        failed: "تعذّر إكمال التشكيل. حاول مرة أخرى لاحقًا.",
        cancelled_unwatched: "أُلغي التشكيل لأن صفحته أُغلقت.",
        cancelled_shutdown: "أُلغي التشكيل أثناء تحديث الموقع. حاول مرة أخرى بعد بضع دقائق.",
        cancel: "إلغاء المهمة",
        cancelling: "جارٍ الإلغاء…",
        cancel_failed: "تعذّر تأكيد الإلغاء. حاول مرة أخرى.",
        download_text: "تنزيل النص (TXT)",
        download_docx: "تنزيل مستند Word (DOCX)",
        download_original: "تنزيل الملف الأصلي",
        another: "تشكيل نص أو مستند آخر",
        result_placeholder: "ستظهر النتيجة هنا.",
        closed: "نحدّث الموقع الآن. حاول مرة أخرى بعد بضع دقائق.",
        full: "الطابور ممتلئ الآن. حاول مرة أخرى بعد بضع دقائق.",
        active_job: "يوجد تشكيل قيد التنفيذ من اتصالك بالإنترنت. انتظر حتى ينتهي أو ألغِه قبل بدء طلب آخر.",
        back_to_job: "العودة إلى التشكيل",
        no_reservation: "انتهت مهلة هذا الطلب. حاول مرة أخرى.",
        missing: "اختر مستندًا أولًا.",
        wrong_type: "نقبل ملف TXT أو DOCX واحدًا فقط. ملفات DOC غير مدعومة.",
        invalid_utf8: "تعذّر قراءة ترميز النص. احفظ ملف TXT بترميز UTF-8 ثم حاول مرة أخرى.",
        invalid_docx: "تعذّر قراءة مستند Word. تأكد أنه ملف DOCX سليم وغير محمي بكلمة مرور.",
        unsupported_docx: "يحتوي مستند Word على عناصر لا نستطيع الحفاظ عليها أثناء التشكيل. احفظ نسخة DOCX بتنسيق أبسط أو حوّل النص إلى ملف TXT.",
        docx_limits: "يتجاوز محتوى مستند Word أو ناتج تشكيله حدود المعالجة. قسّمه إلى مستندات أصغر وفقرات أقصر ثم حاول مرة أخرى.",
        unreadable: "لم نتمكن من قراءة الملف. تأكد من سلامته وحاول مرة أخرى.",
        unavailable: "أداة التشكيل غير متاحة مؤقتًا. حاول مرة أخرى لاحقًا.",
    },
    transcription: TranscriptionText {
        input_formats: "MP3 · M4A",
        output_formats: "TXT · SRT",
        name: "تفريغ الصوت",
        description: "حوّل تسجيلات MP3 وM4A إلى نص مكتوب وملف ترجمة SRT.",
        all_tools: "كل الأدوات",
        transcript_title: "النص",
        file_label: "الملف الصوتي",
        choose_file: "اختر ملفًا صوتيًا، أو اسحبه وأفلته هنا",
        submit: "ابدأ التفريغ",
        uploading: "جارٍ رفع الملف…",
        upload_failed: "تعذّر رفع الملف. تحقّق من اتصالك وحاول مرة أخرى.",
        needs_javascript: "تحتاج هذه الصفحة إلى JavaScript.",
        keep_open: "أبقِ هذه الصفحة مفتوحة حتى ينتهي التفريغ، فإغلاقها يلغيه.",
        reserved: "ننتظر وصول الملف…",
        running: "جارٍ التفريغ…",
        done: "اكتمل التفريغ.",
        download_text: "تنزيل النص (TXT)",
        download_srt: "تنزيل الترجمة (SRT)",
        no_speech: "لم نجد كلامًا في هذا التسجيل.",
        failed: "تعذّر إكمال التفريغ. حاول مرة أخرى لاحقًا.",
        cancelled_unwatched: "أُلغي التفريغ لأن صفحته أُغلقت.",
        cancelled_shutdown: "أُلغي التفريغ أثناء تحديث الموقع. ارفع الملف مرة أخرى بعد بضع دقائق.",
        another: "تفريغ ملف آخر",
        closed: "نحدّث الموقع الآن. حاول مرة أخرى بعد بضع دقائق.",
        full: "الطابور ممتلئ الآن. حاول مرة أخرى بعد بضع دقائق.",
        active_job: "يوجد تفريغ قيد التنفيذ من اتصالك بالإنترنت. انتظر حتى ينتهي، ثم ارفع ملفًا آخر.",
        back_to_job: "العودة إلى تفريغك",
        no_reservation: "انتهت مهلة هذا الرفع. حاول مرة أخرى.",
        missing: "اختر ملفًا صوتيًا أولًا.",
        wrong_type: "نقبل ملفات MP3 وM4A فقط.",
        unreadable: "لم نتمكن من قراءة الملف تسجيلًا صوتيًا بصيغة MP3 أو M4A.",
    },
    ocr: OcrText {
        review: OcrReviewText {
            source_title: "المستند الأصلي",
            text_title: "النص المستخرج",
            view_label: "طريقة العرض",
            source_view: "الأصل",
            text_view: "النص",
            previous_page: "الصفحة السابقة",
            next_page: "الصفحة التالية",
            page_label: "رقم الصفحة",
            page_position: "الصفحة {current} من {total}",
            page_name: "الصفحة {current}",
            page_error: "أدخل رقم صفحة صحيحًا من 1 إلى {total}.",
            fit_width: "ملاءمة العرض",
            fit_page: "ملاءمة الصفحة",
            zoom_in: "تكبير",
            zoom_out: "تصغير",
            zoom_label: "تكبير الأصل",
            copy_page: "نسخ هذه الصفحة",
            source_missing: "اختر الملف الأصلي لاستعادة المعاينة. لن تُعاد عملية استخراج النص.",
            source_confirm: "أؤكد أن هذا هو الملف الأصلي",
            source_file: "ملف PDF أو الصورة الأصلية",
            source_attach: "استعادة المعاينة",
            source_local: "أصل مرفق محليًا",
            source_browser: "الأصل متاح في هذا المتصفح فقط",
            source_loading: "جارٍ تحميل المعاينة المحلية…",
            source_failed: "تعذّر عرض المعاينة المحلية. اختر الملف الأصلي مجددًا للمحاولة. لا يزال النص المستخرج والتنزيلات متاحين.",
            source_mismatch: "عدد صفحات هذا الملف {actual}، وعدد صفحات النتيجة {total}. اختر الملف الأصلي.",
            source_required: "اختر الملف الأصلي وأكّد ذلك قبل استعادة المعاينة.",
            all_blank: "لم نجد نصًا في هذا المستند. يمكنك مراجعة صفحاته.",
        },
        name: "استخراج النص من الصور",
        description: "حوّل ملفات PDF وصور JPG وPNG إلى نص قابل للتحرير، بأي لغة، ونزّله بصيغ TXT وDOCX وJSON.",
        input_formats: "PDF · JPG · PNG",
        output_formats: "TXT · DOCX · JSON",
        all_tools: "كل الأدوات",
        preview_title: "النص صفحةً صفحة",
        file_label: "ملف PDF أو صورة",
        choose_file: "اختر ملف PDF أو صورة، أو اسحبه وأفلته هنا",
        submit: "ابدأ استخراج النص",
        uploading: "جارٍ رفع الملف…",
        upload_failed: "تعذّر رفع الملف. تحقّق من اتصالك وحاول مرة أخرى.",
        needs_javascript: "تحتاج هذه الصفحة إلى JavaScript.",
        keep_open: "أبقِ هذه الصفحة مفتوحة حتى ينتهي استخراج النص، فإغلاقها يلغيه.",
        reserved: "ننتظر وصول الملف…",
        running: "جارٍ استخراج النص…",
        done: "اكتمل استخراج النص.",
        download_text: "تنزيل النص (TXT)",
        download_docx: "تنزيل مستند Word (DOCX)",
        download_json: "تنزيل بيانات الصفحات (JSON)",
        no_text: "لم نجد نصًا في هذه الصفحة.",
        failed: "تعذّر إكمال استخراج النص. حاول مرة أخرى لاحقًا.",
        cancelled_unwatched: "أُلغي استخراج النص لأن صفحته أُغلقت.",
        cancelled_shutdown: "أُلغي استخراج النص أثناء تحديث الموقع. ارفع الملف مرة أخرى بعد بضع دقائق.",
        another: "استخراج النص من ملف آخر",
        closed: "نحدّث الموقع الآن. حاول مرة أخرى بعد بضع دقائق.",
        full: "الطابور ممتلئ الآن. حاول مرة أخرى بعد بضع دقائق.",
        active_job: "يوجد استخراج نص قيد التنفيذ من اتصالك بالإنترنت. انتظر حتى ينتهي، ثم ارفع ملفًا آخر.",
        back_to_job: "العودة إلى ملفك",
        no_reservation: "انتهت مهلة هذا الرفع. حاول مرة أخرى.",
        missing: "اختر ملف PDF أو صورة أولًا.",
        wrong_type: "نقبل ملف PDF واحدًا أو صورة JPG أو PNG واحدة فقط.",
        unreadable: "لم نتمكن من قراءة الملف. تأكد من سلامته وأنه غير محمي بكلمة مرور.",
        unavailable: "أداة استخراج النص غير متاحة مؤقتًا. حاول مرة أخرى لاحقًا.",
        privacy: "نرسل الصفحات إلى Google Drive لاستخراج النص ونحذفها منه فورًا بعد ذلك. نحذف ملفك عند انتهاء المهمة، والنتائج بعد 30 دقيقة.",
        kept_for: "تبقى النتائج متاحة لمدة 30 دقيقة.",
    },
};

// Preserve the requested Arabic model spelling and its combining-mark order.
#[allow(clippy::unicode_not_nfc)]
static EN: Text = Text {
    name: "English",
    site_name: "arabi.tools",
    home_label: "Homepage",
    skip_to_main: "Skip to main content",
    tagline: "Free tools for the Arabic language",
    lead: "Simple tools for working in Arabic. Upload a file and get the result, with no account and nothing to install.",
    description: "Free tools for audio transcription, extracting text from PDFs and images, and Arabic diacritization, with no account and nothing to install.",
    preview_alt: "The arabi.tools logo above the words: Free tools for the Arabic language. Audio transcription · PDF/image text recognition · Automatic diacritization",
    free: "Free, no account",
    files_deleted: "Files deleted automatically",
    arabic_first: "Arabic first",
    tools_title: "Tools",
    open_tool: "Open tool",
    more_tools: "More tools are on the way.",
    source_code: "Source code",
    initiative_before: " is a project of the ",
    initiative_name: "iEasyBooks",
    initiative_after: " initiative.",
    social: SocialText {
        label: "Social links",
        telegram: Account {
            name: "Telegram",
            label: "Our Telegram channel",
        },
        youtube: Account {
            name: "YouTube",
            label: "Our YouTube channel",
        },
        x: Account {
            name: "X",
            label: "Our X account",
        },
        facebook: Account {
            name: "Facebook",
            label: "Our Facebook page",
        },
        github: Account {
            name: "GitHub",
            label: "Our GitHub account for developers",
        },
        email: Account {
            name: "Email",
            label: "Our email",
        },
    },
    not_found_title: "Page not found",
    not_found: "This page doesn't exist, or it has expired.",
    back_home: "Back to home",
    cancelled_user: "You cancelled this job.",
    workspace: WorkspaceText {
        reserving: "Requesting a place in the queue…",
        validating: "Upload complete. Checking the file…",
        observing: "Waiting for job status…",
        delayed: "Status updates are delayed. You can retry or open this job.",
        retry_observation: "Retry status updates",
        recovery: "Open this job",
        view_result: "View result",
        previous_result: "Previous result",
        previous_model: "Previous result — {model}",
        replace_file: "Replace file",
        remove_file: "Remove file",
        bytes: "bytes",
        kilobytes: "KB",
        megabytes: "MB",
        language_reload: "Changing language reloads this page. Unsent changes and the local preview may be lost.",
        source_needed: "Paste your text or select your file again to start another job.",
        missing: "This job is unavailable or has expired. Text already shown here can still be copied.",
        result: "Result",
    },
    review: ReviewText {
        search: "Search recognized text",
        ignore_marks: "Ignore common diacritics",
        ignore_tatweel: "Ignore tatweel",
        match_count: "Match {current} of {total}",
        no_matches: "No matches found",
        previous_match: "Previous match",
        next_match: "Next match",
        clear_search: "Clear search",
        searching: "Searching…",
        search_unavailable: "Search needs a newer browser. You can still select, copy, and download the text.",
        copy_all: "Copy all text",
        download: "Download",
        copy_cue: "Copy this cue",
        copied: "Text copied",
        copy_failed: "Copying was unavailable. Select the text and copy it manually.",
        copy_label: "Text to copy",
        copy_close: "Close copy panel",
        show_timestamps: "Show timestamps",
        go_to_cue: "Go to cue",
        cue_at: "Cue {number} at {time}",
    },
    diacritization: DiacritizationText {
        name: "Arabic diacritization",
        description: "Add diacritics to Arabic text or a TXT or DOCX document using شكِّلها or CATT.",
        input_formats: "Text · TXT · DOCX",
        output_formats: "Diacritized text · TXT · DOCX",
        all_tools: "All tools",
        replaces_marks: "Existing common diacritics are replaced with new ones. Review the result before using it.",
        input_mode: "Input method",
        text_mode: "Text",
        document_mode: "Document",
        model: "Diacritization model",
        shakkelha: "شكِّلها",
        catt: "CATT",
        choose_model: "Choose a diacritization model: شكِّلها or CATT.",
        model_unavailable: "The selected model is temporarily unavailable. Try the other model or return later.",
        source_label: "Original text",
        placeholder: "Type or paste Arabic text here…",
        character_count: "{count} / {max} characters",
        text_missing: "Enter some text first.",
        result_label: "Diacritized text",
        submit_text: "Diacritize text",
        submit_document: "Diacritize document",
        file_label: "TXT or DOCX file",
        choose_file: "Choose a document, or drop it here",
        uploading: "Uploading…",
        submitting: "Sending text…",
        upload_failed: "The request failed. Check your connection and try again.",
        needs_javascript: "This page needs JavaScript.",
        keep_open: "Keep this page open until diacritization finishes. Closing it cancels the job.",
        reserved: "Waiting for your text or document…",
        running: "Adding diacritics…",
        done: "Diacritization complete.",
        failed: "Diacritization could not be completed. Please try again later.",
        cancelled_unwatched: "Diacritization was cancelled because its page was closed.",
        cancelled_shutdown: "Diacritization was cancelled while we updated the site. Please try again in a few minutes.",
        cancel: "Cancel job",
        cancelling: "Cancelling…",
        cancel_failed: "We could not confirm cancellation. Please try again.",
        download_text: "Download text (.txt)",
        download_docx: "Download Word document (.docx)",
        download_original: "Download original file",
        another: "Diacritize another text or document",
        result_placeholder: "The result will appear here.",
        closed: "We are updating the site. Please try again in a few minutes.",
        full: "The queue is full right now. Please try again in a few minutes.",
        active_job: "Diacritization from your connection is already in progress. Wait for it to finish or cancel it before starting another request.",
        back_to_job: "Back to your diacritization",
        no_reservation: "This request expired. Please try again.",
        missing: "Choose a document first.",
        wrong_type: "Choose exactly one TXT or DOCX file. DOC files are not supported.",
        invalid_utf8: "We could not read the text encoding. Save the TXT file as UTF-8 and try again.",
        invalid_docx: "We could not read this Word document. Check that it is a valid DOCX file and is not password protected.",
        unsupported_docx: "This Word document contains content we cannot preserve during diacritization. Save a simpler DOCX copy or convert the text to TXT.",
        docx_limits: "This Word document's content or diacritized output exceeds processing limits. Split it into smaller documents and shorter paragraphs, then try again.",
        unreadable: "We could not read this file. Check that it is valid and try again.",
        unavailable: "Arabic diacritization is temporarily unavailable. Please try again later.",
    },
    transcription: TranscriptionText {
        input_formats: "MP3 · M4A",
        output_formats: "TXT · SRT",
        name: "Audio transcription",
        description: "Turn MP3 and M4A recordings into text and SRT subtitles.",
        all_tools: "All tools",
        transcript_title: "Transcript",
        file_label: "Audio file",
        choose_file: "Choose an audio file, or drop it here",
        submit: "Start transcription",
        uploading: "Uploading…",
        upload_failed: "The upload failed. Check your connection and try again.",
        needs_javascript: "This page needs JavaScript.",
        keep_open: "Keep this page open until the transcription is done. Closing it cancels it.",
        reserved: "Waiting for your file…",
        running: "Transcribing…",
        done: "Transcription complete.",
        download_text: "Download text (.txt)",
        download_srt: "Download subtitles (.srt)",
        no_speech: "We didn't find any speech in this recording.",
        failed: "The transcription couldn't be completed. Please try again later.",
        cancelled_unwatched: "The transcription was cancelled because its page was closed.",
        cancelled_shutdown: "The transcription was cancelled while we updated the site. Please upload the file again in a few minutes.",
        another: "Transcribe another file",
        closed: "We're updating the site. Please try again in a few minutes.",
        full: "The queue is full right now. Please try again in a few minutes.",
        active_job: "A transcription from your connection is already in progress. Wait for it to finish, then upload another file.",
        back_to_job: "Back to your transcription",
        no_reservation: "This upload expired. Please try again.",
        missing: "Choose an audio file first.",
        wrong_type: "Only MP3 and M4A files are accepted.",
        unreadable: "We couldn't read this file as MP3 or M4A audio.",
    },
    ocr: OcrText {
        review: OcrReviewText {
            source_title: "Original document",
            text_title: "Recognized text",
            view_label: "Review view",
            source_view: "Source",
            text_view: "Text",
            previous_page: "Previous page",
            next_page: "Next page",
            page_label: "Page number",
            page_position: "Page {current} of {total}",
            page_name: "Page {current}",
            page_error: "Enter a whole page number from 1 to {total}.",
            fit_width: "Fit to width",
            fit_page: "Fit page",
            zoom_in: "Zoom in",
            zoom_out: "Zoom out",
            zoom_label: "Source zoom",
            copy_page: "Copy this page",
            source_missing: "Select the original file to restore the preview. Recognition will not run again.",
            source_confirm: "I confirm this is the original file",
            source_file: "Original PDF or image",
            source_attach: "Restore preview",
            source_local: "Locally attached source",
            source_browser: "Source available in this browser only",
            source_loading: "Loading local preview…",
            source_failed: "The local preview could not be displayed. Select the original file again to retry. Your recognized text and downloads are still available.",
            source_mismatch: "This file has {actual} pages; the result has {total}. Select the original file.",
            source_required: "Select the original file and confirm it before restoring the preview.",
            all_blank: "No text was found in this document. You can still review its pages.",
        },
        name: "Text recognition (OCR)",
        description: "Turn PDFs and JPG or PNG images in any language into editable text, with TXT, DOCX and JSON downloads.",
        input_formats: "PDF · JPG · PNG",
        output_formats: "TXT · DOCX · JSON",
        all_tools: "All tools",
        preview_title: "Text by page",
        file_label: "PDF or image file",
        choose_file: "Choose a PDF or image, or drop it here",
        submit: "Extract text",
        uploading: "Uploading…",
        upload_failed: "The upload failed. Check your connection and try again.",
        needs_javascript: "This page needs JavaScript.",
        keep_open: "Keep this page open until text recognition finishes. Closing it cancels the job.",
        reserved: "Waiting for your file…",
        running: "Recognizing text…",
        done: "Text recognition complete.",
        download_text: "Download text (.txt)",
        download_docx: "Download Word document (.docx)",
        download_json: "Download pages (.json)",
        no_text: "No text was found on this page.",
        failed: "Text recognition could not be completed. Please try again later.",
        cancelled_unwatched: "Text recognition was cancelled because its page was closed.",
        cancelled_shutdown: "Text recognition was cancelled while we updated the site. Please upload the file again in a few minutes.",
        another: "Recognize another file",
        closed: "We are updating the site. Please try again in a few minutes.",
        full: "The queue is full right now. Please try again in a few minutes.",
        active_job: "Text recognition from your connection is already in progress. Wait for it to finish, then upload another file.",
        back_to_job: "Back to your document",
        no_reservation: "This upload expired. Please try again.",
        missing: "Choose a PDF or image first.",
        wrong_type: "Choose exactly one PDF, JPG or PNG file.",
        unreadable: "We could not read this file. Check that it is valid and is not password protected.",
        unavailable: "Text recognition is temporarily unavailable. Please try again later.",
        privacy: "Pages are sent to Google Drive for OCR and deleted right afterward. We delete your upload when the job ends and results after 30 minutes.",
        kept_for: "Results stay available for 30 minutes.",
    },
};
