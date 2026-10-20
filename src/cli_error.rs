use std::{io, process};

pub fn exit(result: Result<(), Box<dyn std::error::Error>>) -> ! {
    match result {
        Ok(()) => process::exit(0),
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|value| value.kind() == io::ErrorKind::Interrupted) =>
        {
            eprintln!("[opensite-render] cancelled by user");
            process::exit(130);
        }
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    }
}
