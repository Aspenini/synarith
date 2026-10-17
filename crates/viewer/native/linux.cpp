// Only Qt Widgets from the Linux system installation; no Qt sources or plugins bundled.
#include <QtWidgets>
#include "widgets.h"

namespace {
QWidget *window;
QWidget *fields[FieldCount];
QLabel *status;
QPlainTextEdit *log_box;
QPushButton *cancel_button;
Callback handler;
void *state;
bool active = false;
class PreviewLabel : public QLabel {
    QPixmap original;
public:
    explicit PreviewLabel(QWidget *parent) : QLabel(parent) {
        setAlignment(Qt::AlignCenter);
        setMinimumSize(300, 300);
        setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Expanding);
        setAutoFillBackground(true);
        QPalette colors = palette(); colors.setColor(QPalette::Window, Qt::white); setPalette(colors);
        setText("Open a .syar document and render a preview");
    }
    void image(const QImage &image) { original = QPixmap::fromImage(image); fit(); }
    void fit() { if (!original.isNull()) setPixmap(original.scaled(size(), Qt::KeepAspectRatio, Qt::SmoothTransformation)); }
protected:
    void resizeEvent(QResizeEvent *event) override { QLabel::resizeEvent(event); fit(); }
};
PreviewLabel *preview;
QString text(const char *value) { return QString::fromUtf8(value); }
QLineEdit *edit(int field, const char *value) {
    auto *input = new QLineEdit(text(value)); input->setMaxLength(65535); fields[field] = input;
    QObject::connect(input, &QLineEdit::textEdited, window, [] { if (active) handler(state, Changed); });
    return input;
}
QComboBox *choice(int field, std::initializer_list<const char *> values) {
    auto *input = new QComboBox; fields[field] = input;
    for (auto value : values) input->addItem(text(value));
    QObject::connect(input, &QComboBox::currentIndexChanged, window, [] { if (active) handler(state, Changed); });
    return input;
}
QPushButton *button(const char *label, int action) {
    auto *result = new QPushButton(text(label));
    QObject::connect(result, &QPushButton::clicked, window, [action] { handler(state, action); });
    return result;
}
}
extern "C" int syar_ui_run(Callback callback, void *context, const char *source) {
    handler = callback; state = context;
    int argc = 1; char name[] = "syar-view"; char *argv[] = {name, nullptr};
    QApplication app(argc, argv); app.setApplicationName("Synarith Viewer");
    QWidget root; window = &root; root.setWindowTitle("Synarith Viewer"); root.resize(1200, 800);
    auto *layout = new QVBoxLayout(&root);
    auto *file = new QHBoxLayout;
    file->addWidget(new QLabel("Document")); file->addWidget(edit(Source, source), 1); file->addWidget(button("Open…", OpenAction));
    layout->addLayout(file);
    auto *body = new QHBoxLayout;
    auto *panel = new QWidget; panel->setFixedWidth(330); auto *controls = new QVBoxLayout(panel);
    controls->setContentsMargins(0, 8, 8, 0);
    auto *form = new QFormLayout;
    form->addRow("Width", edit(Width, "1024"));
    form->addRow("Height", edit(Height, "1024"));
    form->addRow("Time (seconds)", edit(Time, "0"));
    form->addRow("Samples / axis", choice(Samples, {"1", "2", "4", "8"}));
    form->addRow("Backend", choice(Backend, {"auto", "cpu", "gpu"}));
    form->addRow("Output name", edit(Output, "main"));
    form->addRow("Overrides (;)", edit(Parameters, ""));
    fields[Parameters]->setToolTip("NAME=VALUE; other=vec2(1.0, 2.0)");
    form->addRow("Format", choice(Format, {"png", "jpeg", "bmp", "tga", "ppm"}));
    form->addRow("JPEG quality", edit(Quality, "90"));
    form->addRow("Seed (optional)", edit(Seed, ""));
    controls->addLayout(form);
    auto *actions = new QHBoxLayout;
    auto *render = button("Preview", PreviewAction); render->setDefault(true);
    actions->addWidget(render); actions->addWidget(button("Export…", ExportAction));
    cancel_button = button("Cancel", CancelAction); cancel_button->setEnabled(false); actions->addWidget(cancel_button);
    controls->addLayout(actions);
    status = new QLabel("Choose a document. All rendering runs through syar."); status->setWordWrap(true);
    status->setTextFormat(Qt::PlainText); status->setMinimumHeight(65); status->setTextInteractionFlags(Qt::TextSelectableByMouse);
    controls->addWidget(status);
    log_box = new QPlainTextEdit; log_box->setReadOnly(true); controls->addWidget(log_box, 1);
    body->addWidget(panel); preview = new PreviewLabel(&root); body->addWidget(preview, 1); layout->addLayout(body, 1);
    QShortcut open(QKeySequence::Open, &root), refresh(QKeySequence("Ctrl+R"), &root), cancel(QKeySequence(Qt::Key_Escape), &root);
    QObject::connect(&open, &QShortcut::activated, &root, [] { handler(state, OpenAction); });
    QObject::connect(&refresh, &QShortcut::activated, &root, [] { handler(state, PreviewAction); });
    QObject::connect(&cancel, &QShortcut::activated, &root, [] { handler(state, CancelAction); });
    QTimer timer;
    QObject::connect(&timer, &QTimer::timeout, &root, [] { handler(state, Tick); });
    timer.start(40); active = true; root.show();
    QTimer::singleShot(0, &root, [] { handler(state, Ready); });
    int result = app.exec();
    active = false; timer.stop(); handler(state, CloseAction); window = nullptr;
    return result;
}
extern "C" size_t syar_ui_get(int field, char *buffer, size_t capacity) {
    if (field < 0 || field >= FieldCount) return 0;
    QString value;
    if (auto *input = qobject_cast<QLineEdit *>(fields[field])) value = input->text();
    else value = static_cast<QComboBox *>(fields[field])->currentText();
    return copy_text(value.toUtf8().toStdString(), buffer, capacity);
}
extern "C" void syar_ui_set(int field, const char *value) {
    if (field < 0 || field >= FieldCount) return;
    QSignalBlocker blocked(fields[field]);
    if (auto *input = qobject_cast<QLineEdit *>(fields[field])) input->setText(text(value));
    else static_cast<QComboBox *>(fields[field])->setCurrentText(text(value));
}
extern "C" void syar_ui_status(const char *message, bool busy) { status->setText(text(message)); cancel_button->setEnabled(busy); }
extern "C" void syar_ui_log(const char *message) { log_box->setPlainText(text(message)); }
extern "C" void syar_ui_image(const uint8_t *rgba, int width, int height) {
    preview->image(QImage(rgba, width, height, width * 4, QImage::Format_RGBA8888).copy());
}
extern "C" bool syar_ui_choose(bool save, const char *suggested, const char *extension, char *buffer, size_t capacity) {
    QFileDialog dialog(window, save ? "Export Synarith image" : "Open Synarith document");
    dialog.setAcceptMode(save ? QFileDialog::AcceptSave : QFileDialog::AcceptOpen);
    dialog.setFileMode(save ? QFileDialog::AnyFile : QFileDialog::ExistingFile);
    // Rust confirms the final, normalized filename once before invoking the CLI.
    dialog.setOption(QFileDialog::DontConfirmOverwrite, true);
    dialog.setDefaultSuffix(text(extension));
    dialog.setNameFilter(save ? QString("Image (*.%1);;All files (*)").arg(text(extension)) : "Synarith documents (*.syar);;All files (*)");
    dialog.selectFile(text(suggested));
    if (dialog.exec() != QDialog::Accepted || dialog.selectedFiles().isEmpty()) return false;
    auto selected = dialog.selectedFiles().first().toUtf8().toStdString();
    if (selected.size() + 1 > capacity) return false;
    copy_text(selected, buffer, capacity); return true;
}
extern "C" bool syar_ui_confirm(const char *message) {
    QMessageBox dialog(QMessageBox::Question, "Synarith Viewer", text(message), QMessageBox::Yes | QMessageBox::No, window);
    dialog.setTextFormat(Qt::PlainText); dialog.setDefaultButton(QMessageBox::No); return dialog.exec() == QMessageBox::Yes;
}
extern "C" void syar_ui_quit() { QTimer::singleShot(0, window, [] { window->close(); }); }
extern "C" size_t syar_ui_capture(uint8_t *rgba, size_t capacity, int *width, int *height) {
    auto image = window->grab().toImage().convertToFormat(QImage::Format_RGBA8888);
    *width = image.width(); *height = image.height(); size_t size = (size_t)*width * *height * 4;
    if (!rgba) return size;
    if (capacity < size) return 0;
    for (int y = 0; y < *height; ++y) std::memcpy(rgba + (size_t)y * *width * 4, image.constScanLine(y), (size_t)*width * 4);
    return size;
}
