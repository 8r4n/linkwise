//! One-sided postcard artwork in printer points, with a vector QR symbol.
use chrono::{DateTime, SecondsFormat, Utc};
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str};
use qrcode::{Color, QrCode};

const SANS: Name = Name(b"Sans");
const BOLD: Name = Name(b"Bold");
const MONO: Name = Name(b"Mono");
const INK: (f32, f32, f32) = (0.08, 0.15, 0.18);
const ACCENT: (f32, f32, f32) = (0.04, 0.35, 0.39);

pub(super) struct Contact {
    pub utc: DateTime<Utc>,
    pub frequency_khz: String,
    pub mode: String,
    pub rst: String,
    pub grid: Option<String>,
    pub notes: Option<String>,
}

#[derive(Clone, Copy, Default)]
pub(super) enum CardSize {
    #[default]
    Usps,
    FourBySix,
}

impl CardSize {
    fn points(self) -> (f32, f32) {
        match self {
            Self::Usps => (396., 252.),
            Self::FourBySix => (432., 288.),
        }
    }
}

fn text(content: &mut Content, x: f32, y: f32, font: Name, size: f32, value: &str) {
    content
        .begin_text()
        .set_font(font, size)
        .set_text_matrix([1., 0., 0., 1., x, y])
        .show(Str(value.as_bytes()))
        .end_text();
}

pub(super) fn render(
    size: CardSize,
    from: &str,
    to: &str,
    contact: &Contact,
    encrypted_mail: bool,
    qr: &QrCode,
) -> Vec<u8> {
    let (width, height) = size.points();
    let margin = 23.;
    let side = height - 2. * margin;
    let qr_left = width - margin - side;
    let left_width = qr_left - margin - 11.;
    let mut contents = Content::new();
    contents.set_fill_rgb(ACCENT.0, ACCENT.1, ACCENT.2);
    text(&mut contents, margin, height - 33., BOLD, 11., "QSL CARD");
    contents
        .rect(margin, height - 42., left_width, 0.8)
        .fill_nonzero();
    contents.set_fill_rgb(INK.0, INK.1, INK.2);
    let call_size = 27_f32.min(left_width / (0.98 * from.len() as f32));
    text(&mut contents, margin, height - 72., BOLD, call_size, from);
    text(
        &mut contents,
        margin,
        height - 89.,
        BOLD,
        if to.len() > 8 { 8. } else { 9. },
        &format!("CONFIRMS QSO WITH {to}"),
    );
    let dt = contact.utc.to_rfc3339_opts(SecondsFormat::Secs, true);
    let time = format!("{}   {} UTC", &dt[..10], &dt[11..19]);
    text(&mut contents, margin, height - 111., SANS, 10., &time);
    text(
        &mut contents,
        margin,
        height - 127.,
        SANS,
        if contact.frequency_khz.len() + contact.mode.len() > 13 {
            8.
        } else {
            10.
        },
        &format!("{} kHz  /  {}", contact.frequency_khz, contact.mode),
    );
    let rst = match &contact.grid {
        Some(grid) => format!("RST {}  /  GRID {grid}", contact.rst),
        None => format!("RST {}", contact.rst),
    };
    text(&mut contents, margin, height - 143., SANS, 10., &rst);
    if let Some(notes) = &contact.notes {
        let mut remaining = notes.trim();
        let mut row = 0;
        // Remarks are ASCII. At 8 points, 26 Courier cells fit the text area.
        while !remaining.is_empty() {
            let end = if remaining.len() <= 26 {
                remaining.len()
            } else {
                remaining[..26].rfind(' ').unwrap_or(26)
            };
            let (line, rest) = remaining.split_at(end);
            text(
                &mut contents,
                margin,
                height - 158. - 9. * row as f32,
                MONO,
                8.,
                line,
            );
            remaining = rest.trim_start();
            row += 1;
        }
    }

    contents.set_fill_rgb(ACCENT.0, ACCENT.1, ACCENT.2);
    text(
        &mut contents,
        margin,
        37.,
        BOLD,
        8.,
        "LINKWISE PUBLIC CERTIFICATES",
    );
    let label = if encrypted_mail {
        "Signing and mail encryption"
    } else {
        "Signing / chat identity"
    };
    text(&mut contents, margin, 25., SANS, 8., label);

    // Each dark run is one rectangle. The 4-module quiet zone is part of the square.
    let count = qr.width();
    let pitch = side / (count + 8) as f32;
    contents
        .set_fill_gray(1.)
        .rect(qr_left, margin, side, side)
        .fill_nonzero();
    contents.set_fill_gray(0.);
    for row in 0..count {
        let mut column = 0;
        while column < count {
            if qr[(column, row)] == Color::Light {
                column += 1;
                continue;
            }
            let start = column;
            while column < count && qr[(column, row)] == Color::Dark {
                column += 1;
            }
            contents.rect(
                qr_left + (4 + start) as f32 * pitch,
                margin + (count + 3 - row) as f32 * pitch,
                (column - start) as f32 * pitch,
                pitch,
            );
        }
    }
    contents.fill_nonzero();

    let mut pdf = Pdf::new();
    let catalog = Ref::new(1);
    let pages = Ref::new(2);
    let page = Ref::new(3);
    let regular = Ref::new(4);
    let bold = Ref::new(5);
    let stream = Ref::new(6);
    let mono = Ref::new(7);
    pdf.catalog(catalog).pages(pages);
    pdf.pages(pages).kids([page]).count(1);
    let mut sheet = pdf.page(page);
    sheet
        .parent(pages)
        .media_box(Rect::new(0., 0., width, height))
        .contents(stream);
    sheet
        .resources()
        .fonts()
        .pair(SANS, regular)
        .pair(BOLD, bold)
        .pair(MONO, mono);
    sheet.finish();
    pdf.type1_font(regular).base_font(Name(b"Helvetica"));
    pdf.type1_font(bold).base_font(Name(b"Helvetica-Bold"));
    pdf.type1_font(mono).base_font(Name(b"Courier"));
    pdf.stream(stream, &contents.finish());
    pdf.finish()
}
