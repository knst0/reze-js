fn main() {
    reze_build::compile(
        reze_build::Config::new("..").types("../types").chunks(reze_build::Chunks::Component),
    );
}
