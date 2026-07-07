use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use voicewave_core_lib::dictionary::DictionaryManager;
use voicewave_core_lib::history::{HistoryManager, SessionHistoryQuery};
use voicewave_core_lib::insertion::{
    BackendInsertSuccess, InsertTextRequest, InsertionBackend, InsertionEngine, UndoToken,
};

static LAST_INSERTED_TEXT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static LONG_UTTERANCE_TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

fn capture_slot() -> &'static Mutex<Option<String>> {
    LAST_INSERTED_TEXT.get_or_init(|| Mutex::new(None))
}

fn long_utterance_test_mutex() -> &'static Mutex<()> {
    LONG_UTTERANCE_TEST_MUTEX.get_or_init(|| Mutex::new(()))
}

#[derive(Default)]
struct CaptureBackend;

impl InsertionBackend for CaptureBackend {
    fn detect_target_app(&mut self) -> Option<String> {
        Some("Notepad".to_string())
    }

    fn direct_insert(
        &mut self,
        text: &str,
        _target_app: Option<&str>,
    ) -> Result<BackendInsertSuccess, String> {
        *capture_slot()
            .lock()
            .expect("capture mutex should be available") = Some(text.to_string());
        Ok(BackendInsertSuccess {
            message: None,
            undo_token: Some(UndoToken::KeyboardUndo),
        })
    }

    fn clipboard_paste(
        &mut self,
        text: &str,
        target_app: Option<&str>,
    ) -> Result<BackendInsertSuccess, String> {
        self.direct_insert(text, target_app)
    }

    fn clipboard_only(
        &mut self,
        text: &str,
        target_app: Option<&str>,
    ) -> Result<BackendInsertSuccess, String> {
        self.direct_insert(text, target_app)
    }

    fn undo(&mut self, _token: &UndoToken) -> Result<Option<String>, String> {
        Ok(Some("undo ok".to_string()))
    }
}

fn make_temp_path(stem: &str, extension: &str) -> std::path::PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!("voicewave-{stem}-{ts}.{extension}"))
}

fn build_long_utterance(target_seconds: usize, suffix_terms: &[&str]) -> String {
    // Roughly 2.5 words/sec cadence for dictation simulation.
    let target_words = target_seconds * 25 / 10;
    let mut words = Vec::with_capacity(target_words + suffix_terms.len());
    let corpus = [
        "this",
        "phase",
        "five",
        "reliability",
        "pass",
        "keeps",
        "dictation",
        "stable",
        "through",
        "long",
        "sentences",
        "without",
        "tail",
        "loss",
    ];
    for idx in 0..target_words {
        words.push(corpus[idx % corpus.len()].to_string());
    }
    for token in suffix_terms {
        words.push((*token).to_string());
    }
    words.join(" ")
}

fn assert_paths_receive_full_transcript(transcript: &str, suffix_terms: &[&str]) {
    let _serial_guard = match long_utterance_test_mutex().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    // Insertion path: backend receives full text without truncation.
    *capture_slot()
        .lock()
        .expect("capture mutex should be available") = None;
    let mut insertion = InsertionEngine::new(Box::new(CaptureBackend));
    let insert_result = insertion
        .insert_text(InsertTextRequest {
            text: transcript.to_string(),
            target_app: Some("Notepad".to_string()),
            prefer_clipboard: false,
            force_clipboard_only: false,
        })
        .expect("insertion should succeed");
    assert!(insert_result.success);
    let captured = capture_slot()
        .lock()
        .expect("capture mutex should be available")
        .clone()
        .expect("captured transcript should exist");
    assert_eq!(captured, transcript);

    // History path: long transcript is accepted and persisted without runtime failure.
    let history_path = make_temp_path("history-long-utterance", "json");
    let key_path = make_temp_path("history-long-utterance-key", "key");
    let mut history =
        HistoryManager::from_paths(&history_path, &key_path).expect("history manager should init");
    history
        .record_transcript(transcript)
        .expect("history should record long transcript");
    let records = history.get_records(SessionHistoryQuery {
        limit: Some(1),
        include_failed: Some(true),
        search_query: None,
        tags: None,
        starred: None,
    });
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].source, "dictation");

    // Dictionary path: suffix terms from the end of long utterance must be ingested
    // when the runtime marks the utterance as low-confidence.
    let dictionary_path = make_temp_path("dictionary-long-utterance", "json");
    let mut dictionary =
        DictionaryManager::from_path(&dictionary_path).expect("dictionary manager should init");
    dictionary
        .ingest_transcript_with_signal(transcript, true)
        .expect("dictionary ingestion should succeed");
    let queue = dictionary.get_queue(Some(20));
    for token in suffix_terms {
        assert!(
            queue.iter().any(|entry| entry.term == *token),
            "expected suffix token '{token}' to reach dictionary ingest path"
        );
    }

    let _ = std::fs::remove_file(history_path);
    let _ = std::fs::remove_file(key_path);
    let _ = std::fs::remove_file(dictionary_path);
}

#[test]
fn long_utterance_20s_keeps_suffix_and_reaches_all_paths() {
    let suffix = ["TailAlphaTwentySeconds", "TailOmegaTwentySeconds"];
    let transcript = build_long_utterance(20, &suffix);
    assert!(transcript.ends_with("TailAlphaTwentySeconds TailOmegaTwentySeconds"));
    assert_paths_receive_full_transcript(&transcript, &suffix);
}

#[test]
fn long_utterance_30s_keeps_suffix_and_reaches_all_paths() {
    let suffix = ["TailAlphaThirtySeconds", "TailOmegaThirtySeconds"];
    let transcript = build_long_utterance(30, &suffix);
    assert!(transcript.ends_with("TailAlphaThirtySeconds TailOmegaThirtySeconds"));
    assert_paths_receive_full_transcript(&transcript, &suffix);
}
