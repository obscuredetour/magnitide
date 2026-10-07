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
