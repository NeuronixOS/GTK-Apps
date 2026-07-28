
configure_file(
    input: 'gtk-meld',
    output: 'gtk-meld',
    command: ['../meson_shebang_normalisation.py', '@INPUT@', '@OUTPUT@'] + (is_wheel ? ['--wheel'] : []),
    install_dir: bindir,
)
