//! Text shown to visitors, in Arabic and English. Arabic is the default; a cookie remembers
//! the visitor's choice.

use topcoat::context::Cx;
use topcoat::cookie::{Cookies, cookies};

use crate::queue::Rejection;
use crate::tools::transcription::{AudioProblem, KEEP_FINISHED, MAX_DURATION, MAX_UPLOAD_BYTES};

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

    /// The name of the transcription language `code`, for the language picker.
    pub fn language_name(self, code: &str) -> &str {
        match (self, code) {
            (Self::Ar, "ar") => "العربية",
            (Self::Ar, "en") => "الإنجليزية",
            (Self::En, "ar") => "Arabic",
            (Self::En, "en") => "English",
            _ => code,
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
            AudioProblem::TooLarge => self.too_large(),
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

    pub fn compression_tip(self) -> String {
        match self {
            Self::Ar => format!(
                "ملفك أكبر من الحد؟ صدّره بصيغة MP3 بمعدل 64 كيلوبت في الثانية وبقناة واحدة (Mono). يبقى الكلام واضحًا، ويتسع التسجيل حتى {} ساعات.",
                max_hours()
            ),
            Self::En => format!(
                "File too big? Re-export it as MP3 at 64 kbps, mono. Speech stays clear, and up to {} hours fits.",
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

    pub fn kept_for(self) -> String {
        match self {
            Self::Ar => format!("يبقى النص متاحًا لمدة {} دقيقة.", keep_minutes()),
            Self::En => format!(
                "The transcript stays available for {} minutes.",
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
    /// The logo link's accessible name.
    pub home_label: &'static str,
    pub tagline: &'static str,
    pub lead: &'static str,
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
    pub not_found_title: &'static str,
    pub not_found: &'static str,
    pub back_home: &'static str,
    pub transcription: TranscriptionText,
}

/// Fixed text of the transcription tool.
pub struct TranscriptionText {
    pub name: &'static str,
    pub description: &'static str,
    pub all_tools: &'static str,
    pub steps_label: &'static str,
    pub step_upload: &'static str,
    pub step_transcribe: &'static str,
    pub step_download: &'static str,
    pub before_you_start: &'static str,
    pub transcript_title: &'static str,
    pub file_label: &'static str,
    pub choose_file: &'static str,
    pub no_file: &'static str,
    pub language_label: &'static str,
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

static AR: Text = Text {
    name: "العربية",
    home_label: "الصفحة الرئيسية",
    tagline: "أدوات مجانية للغة العربية",
    lead: "أدوات بسيطة لمن يعمل بالعربية: ارفع ملفك واحصل على النتيجة، دون حساب أو تثبيت.",
    free: "مجانية بلا حساب",
    files_deleted: "تُحذف ملفاتك تلقائيًا",
    arabic_first: "للعربية أولًا",
    tools_title: "الأدوات",
    open_tool: "افتح الأداة",
    more_tools: "المزيد من الأدوات قريبًا.",
    source_code: "الشيفرة المصدرية",
    initiative_before: " مشروع من مبادرة ",
    initiative_name: "الكتب الميسّرة",
    initiative_after: " غير الربحية.",
    not_found_title: "الصفحة غير موجودة",
    not_found: "هذه الصفحة غير موجودة، أو انتهت صلاحيتها.",
    back_home: "العودة إلى الرئيسية",
    transcription: TranscriptionText {
        name: "تفريغ الصوت",
        description: "حوّل تسجيلات MP3 وM4A إلى نص مكتوب وملف ترجمة SRT.",
        all_tools: "كل الأدوات",
        steps_label: "مراحل التفريغ",
        step_upload: "الرفع",
        step_transcribe: "التفريغ",
        step_download: "التنزيل",
        before_you_start: "قبل أن تبدأ",
        transcript_title: "النص",
        file_label: "الملف الصوتي",
        choose_file: "اختر ملفًا صوتيًا، أو اسحبه وأفلته هنا",
        no_file: "لم تختر ملفًا بعد.",
        language_label: "لغة التسجيل",
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
};

static EN: Text = Text {
    name: "English",
    home_label: "Homepage",
    tagline: "Free tools for the Arabic language",
    lead: "Simple tools for working in Arabic. Upload a file and get the result, with no account and nothing to install.",
    free: "Free, no account",
    files_deleted: "Files deleted automatically",
    arabic_first: "Arabic first",
    tools_title: "Tools",
    open_tool: "Open tool",
    more_tools: "More tools are on the way.",
    source_code: "Source code",
    initiative_before: " is a project of the nonprofit ",
    initiative_name: "iEasyBooks",
    initiative_after: " initiative.",
    not_found_title: "Page not found",
    not_found: "This page doesn't exist, or it has expired.",
    back_home: "Back to home",
    transcription: TranscriptionText {
        name: "Audio transcription",
        description: "Turn MP3 and M4A recordings into text and SRT subtitles.",
        all_tools: "All tools",
        steps_label: "Transcription steps",
        step_upload: "Upload",
        step_transcribe: "Transcribe",
        step_download: "Download",
        before_you_start: "Before you start",
        transcript_title: "Transcript",
        file_label: "Audio file",
        choose_file: "Choose an audio file, or drop it here",
        no_file: "No file chosen yet.",
        language_label: "Recording language",
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
};
