use cm_modes::xml::XmlMode;
use cm_modes::{JsArg, ModeOptions, ModeSpec};
use criterion::{Criterion, criterion_group, criterion_main};

pub fn criterion_benchmark(c: &mut Criterion) {
    let spec = ModeSpec::default();
    let options = ModeOptions::default();
    let mode = XmlMode::new(&options, &spec);

    let line1_u16 = (0..500)
        .map(|_| "<a>")
        .collect::<Vec<_>>()
        .join("")
        .encode_utf16()
        .collect::<Vec<_>>();
    let line2_u16 = (0..500)
        .map(|_| "</a>")
        .collect::<Vec<_>>()
        .join("")
        .encode_utf16()
        .collect::<Vec<_>>();

    let mut state = mode.start_state_js(JsArg::Pass);
    let mut stream = cm_core::util::string_stream::StringStream::new(&line1_u16, Some(4), None);
    while !stream.eol() {
        mode.token_js(&mut stream, &mut state);
    }

    // Now state is deep inside tags
    let close_tag_u16: Vec<u16> = "</".encode_utf16().collect();
    let open_tag_u16: Vec<u16> = "<a".encode_utf16().collect();

    c.bench_function("xml indent close tag", |b| {
        b.iter(|| {
            std::hint::black_box(mode.indent_js(&state, &close_tag_u16, &close_tag_u16));
        })
    });

    c.bench_function("xml indent open tag", |b| {
        b.iter(|| {
            std::hint::black_box(mode.indent_js(&state, &open_tag_u16, &open_tag_u16));
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
