"""The GUI classes plugins subclass, with the attributes calibre's have."""


def test_message_box_has_calibres_attributes(qapp):
    from calibre.gui2.dialogs.message_box import MessageBox

    class ResultsSummaryDialog(MessageBox):
        def __init__(self, parent, caption, msg, det_msg):
            MessageBox.__init__(self, MessageBox.INFO, caption, msg, det_msg=det_msg, parent=parent)
            # What a real plugin does next.
            self.det_msg_toggle.setVisible(bool(det_msg))
            self.gridLayout.addWidget(self.toggle_checkbox, 4, 0, 1, 2)

    d = ResultsSummaryDialog(None, 'Done', 'Success!', 'line 1\nline 2')
    assert d.det_msg_toggle.isVisibleTo(d) and d.det_msg.toPlainText() == 'line 1\nline 2'
    assert d.ctc_button is not None and d.bb is not None and d.msg.text() == 'Success!'
    d.toggle_det_msg()
    assert d.det_msg.isVisibleTo(d)
    d.set_details('')
    assert not d.det_msg_toggle.isVisibleTo(d)
    q = MessageBox(MessageBox.QUESTION, 'Sure?', 'Really?', yes_text='Do it', no_text='Leave it')
    assert q.is_question and q.bb.buttons()


def test_calibre_icon_names_draw_with_lucide(qapp):
    import json

    from conftest import COMPAT

    from calibre.gui2 import icon_from_name
    from qt.core import QIcon

    mapping = json.loads((COMPAT / 'resources' / 'images' / 'calibre-icons.json').read_text(encoding='utf-8'))
    for calibre_name, lucide in mapping['icons'].items():
        assert (COMPAT / 'resources' / 'images' / 'lucide' / f'{lucide}.svg').exists(), lucide
    for name in ('config.png', 'trash.png', 'edit_input.png', 'plus', 'dialog_warning.png'):
        icon = icon_from_name(name)
        assert isinstance(icon, QIcon) and not icon.isNull(), name
        assert not icon.pixmap(16, 16).isNull()
    assert QIcon.ic('search.png').pixmap(24, 24).width() == 24
    # Unmapped names still get the drawn stand-in rather than nothing.
    assert not icon_from_name('no-such-icon.png').isNull()


def test_i_gives_a_real_file_for_calibre_icon_names(qapp):
    import os

    from calibre.utils.resources import get_image_path as I  # the I() built-in
    from qt.core import QIcon, QPixmap

    path = I('dialog_information.png')
    assert os.path.exists(path) and path.endswith('.png')
    assert not QIcon(path).isNull() and not QPixmap(path).isNull()
    assert I('dialog_information.png') == path, 'rendered once'
    with open(I('config.png'), 'rb') as f:
        assert f.read(8) == b'\x89PNG\r\n\x1a\n'
    assert not os.path.exists(I('no-such-icon.png')), 'unmapped names stay missing, as calibre would report'
