fn main() {
    if std::env::args().any(|argument| argument == "--gapplication-service") {
        if let Err(error) = gnil_fm::file_manager_service::run() {
            eprintln!("gnil-fm FileManager1 service failed: {error}");
            std::process::exit(1);
        }
    } else {
        gnil_fm::file_manager::run();
    }
}
