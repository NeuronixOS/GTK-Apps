//! Print and print-preview using GtkPrintOperation + SourceView print compositor.

use gtk4 as gtk;
use gtk::prelude::*;
use sourceview5::prelude::*;

use crate::config::PrintConfig;

pub fn print_document(
    parent: &impl IsA<gtk::Window>,
    buffer: &sourceview5::Buffer,
    view: &sourceview5::View,
    title: &str,
    cfg: &PrintConfig,
    preview: bool,
) {
    let compositor = sourceview5::PrintCompositor::new(buffer);
    compositor.set_print_header(cfg.print_header);
    compositor.set_print_line_numbers(cfg.print_line_numbers);
    compositor.set_highlight_syntax(cfg.print_syntax_highlighting);
    compositor.set_body_font_name(&cfg.print_font_body);
    compositor.set_line_numbers_font_name(Some(&cfg.print_font_numbers));
    compositor.set_header_font_name(Some(&cfg.print_font_header));
    let wrap = match cfg.print_wrap_mode.as_str() {
        "none" => gtk::WrapMode::None,
        "char" => gtk::WrapMode::Char,
        _ => gtk::WrapMode::Word,
    };
    compositor.set_wrap_mode(wrap);
    if cfg.print_header {
        compositor.set_header_format(true, Some(title), None, Some("%N"));
    }

    let op = gtk::PrintOperation::new();
    op.set_job_name(title);
    op.set_allow_async(true);
    op.set_embed_page_setup(true);

    {
        let compositor = compositor.clone();
        op.connect_begin_print(move |op, ctx| {
            while !compositor.paginate(ctx) {}
            op.set_n_pages(compositor.n_pages());
        });
    }
    {
        let compositor = compositor.clone();
        op.connect_draw_page(move |_op, ctx, page| {
            compositor.draw_page(ctx, page);
        });
    }

    let action = if preview {
        gtk::PrintOperationAction::Preview
    } else {
        gtk::PrintOperationAction::PrintDialog
    };

    let _ = op.run(action, Some(parent));
    let _ = view;
}
