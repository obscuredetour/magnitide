"""calibre's conversion pipeline isn't in Magnitide (it converts through
its own code and an installed calibre's `ebook-convert`). This package holds
the few helpers plugins import from it that don't need the pipeline
(`preprocess`); `plumber`, `config` and `cli` say they aren't available."""


class ConversionUserFeedBack(Exception):
    def __init__(self, title, msg, level='info', det_msg=''):
        Exception.__init__(self, msg)
        self.title, self.msg, self.det_msg = title, msg, det_msg
        self.level = level
