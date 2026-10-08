"""Mirrors calibre's `calibre.gui2.progress_indicator`: the spinning
"working" indicator (`ProgressIndicator`) and `WaitPanel`, drawn in Python
(calibre's is C++)."""

from qt.core import QColor, QLabel, QPainter, QPen, QRect, QSize, QSizePolicy, Qt, QTimer, QVBoxLayout, QWidget


def draw_snake_spinner(painter, rect, angle, light, dark):
    painter.setRenderHint(QPainter.RenderHint.Antialiasing)
    if rect.width() > rect.height():
        delta = (rect.width() - rect.height()) // 2
        rect = rect.adjusted(delta, 0, -delta, 0)
    elif rect.height() > rect.width():
        delta = (rect.height() - rect.width()) // 2
        rect = rect.adjusted(0, delta, 0, -delta)
    disc_width = max(3, min(8, rect.width() // 10))
    drawing_rect = QRect(
        rect.x() + disc_width, rect.y() + disc_width, rect.width() - 2 * disc_width, rect.height() - 2 * disc_width
    )
    gap = 60  # degrees
    pen = QPen(light)
    pen.setWidth(disc_width)
    pen.setCapStyle(Qt.PenCapStyle.RoundCap)
    painter.setPen(pen)
    painter.drawArc(drawing_rect, (angle + gap) * 16, (360 - 2 * gap) * 16)
    pen.setColor(dark)
    painter.setPen(pen)
    painter.drawArc(drawing_rect, (angle - gap) * 16, 2 * gap * 16)


class ProgressIndicator(QWidget):
    def __init__(self, parent=None, size=64, interval=10):
        QWidget.__init__(self, parent)
        self.setDisplaySize(size)
        self.angle = 0
        self.timer = QTimer(self)
        self.timer.setInterval(interval)
        self.timer.timeout.connect(self.tick)
        self.light = QColor(Qt.GlobalColor.lightGray)
        self.dark = QColor(Qt.GlobalColor.darkGray)
        self.setSizePolicy(QSizePolicy.Policy.Fixed, QSizePolicy.Policy.Fixed)

    def setDisplaySize(self, size):
        if isinstance(size, int):
            size = QSize(size, size)
        self.display_size = size
        self.setFixedSize(size)
        self.update()

    def displaySize(self):
        return self.display_size

    def sizeHint(self):
        return self.display_size

    def tick(self):
        self.angle = (self.angle - 6) % 360
        self.update()

    def start(self):
        self.timer.start()

    startAnimation = start

    def stop(self):
        self.timer.stop()
        self.angle = 0
        self.update()

    stopAnimation = stop

    def is_animated(self):
        return self.timer.isActive()

    isAnimated = is_animated

    def paintEvent(self, ev):
        if not self.timer.isActive():
            return
        p = QPainter(self)
        try:
            draw_snake_spinner(p, self.rect(), self.angle, self.light, self.dark)
        finally:
            p.end()


class WaitPanel(QWidget):
    """A message with a spinner, for "working…" states."""

    def __init__(self, msg, parent=None, size=256, interval=10):
        QWidget.__init__(self, parent)
        self.l = l = QVBoxLayout(self)
        self.pi = ProgressIndicator(self, size, interval)
        l.addStretch(), l.addWidget(self.pi, 0, Qt.AlignmentFlag.AlignCenter)
        self.la = QLabel(msg)
        f = self.la.font()
        f.setPointSize(28)
        self.la.setFont(f)
        l.addWidget(self.la, 0, Qt.AlignmentFlag.AlignCenter), l.addStretch()

    @property
    def msg(self):
        return self.la.text()

    @msg.setter
    def msg(self, val):
        self.la.setText(val)

    def start(self):
        self.pi.start()

    def stop(self):
        self.pi.stop()


class WaitStack(QWidget):
    def __init__(self, msg, after=None, parent=None, size=256, interval=10):
        from qt.core import QStackedLayout

        QWidget.__init__(self, parent)
        self.l = l = QStackedLayout(self)
        self.wp = WaitPanel(msg, self, size, interval)
        if after is None:
            after = QWidget(self)
        self.after = after
        l.addWidget(self.wp)
        l.addWidget(after)

    @property
    def msg(self):
        return self.wp.msg

    @msg.setter
    def msg(self, val):
        self.wp.msg = val

    def start(self):
        self.l.setCurrentWidget(self.wp)
        self.wp.start()

    def stop(self):
        self.wp.stop()
        self.l.setCurrentWidget(self.after)


__all__ = ['ProgressIndicator', 'WaitPanel', 'WaitStack', 'draw_snake_spinner']
