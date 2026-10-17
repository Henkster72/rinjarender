use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub fn install() -> Result<Arc<AtomicBool>, Box<dyn std::error::Error>> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let handler_flag = Arc::clone(&cancelled);
    ctrlc::set_handler(move || {
        handler_flag.store(true, Ordering::SeqCst);
    })?;
    Ok(cancelled)
}

pub fn requested(flag: &Arc<AtomicBool>) -> bool {
    flag.load(Ordering::SeqCst)
}
