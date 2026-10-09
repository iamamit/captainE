fn main() -> eframe::Result {
    captaine_app::run(std::env::args_os().nth(1).map(Into::into))
}
