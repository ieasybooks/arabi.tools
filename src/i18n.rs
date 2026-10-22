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
    pub tagline: &'static str,
    pub choose_tool: &'static str,
    pub more_tools: &'static str,
    pub not_found: &'static str,
    pub back_home: &'static str,
    pub transcription: TranscriptionText,
}

/// Fixed text of the transcription tool.
pub struct TranscriptionText {
    pub name: &'static str,
    pub description: &'static str,
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
    tagline: "أدوات مجانية للغة العربية",
    choose_tool: "اختر أداة للبدء.",
    more_tools: "المزيد من الأدوات قريبًا.",
    not_found: "هذه الصفحة غير موجودة، أو انتهت صلاحيتها.",
    back_home: "العودة إلى الرئيسية",
    transcription: TranscriptionText {
        name: "تفريغ الصوت",
        description: "حوّل تسجيلات MP3 وM4A إلى نص مكتوب وملف ترجمة SRT.",
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
    tagline: "Free tools for the Arabic language",
    choose_tool: "Pick a tool to get started.",
    more_tools: "More tools are on the way.",
    not_found: "This page doesn't exist, or it has expired.",
    back_home: "Back to home",
    transcription: TranscriptionText {
        name: "Audio transcription",
        description: "Turn MP3 and M4A recordings into text and SRT subtitles.",
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
