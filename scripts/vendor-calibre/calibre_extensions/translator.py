"""calibre_extensions.translator: gettext over .mo data, as calibre's C version."""

import gettext
import io


class Translator(gettext.GNUTranslations):

    def __init__(self, mo_data=None, mo_file=None):
        if mo_data is not None:
            gettext.GNUTranslations.__init__(self, io.BytesIO(mo_data))
        elif mo_file is not None:
            gettext.GNUTranslations.__init__(self, mo_file)
        else:
            gettext.NullTranslations.__init__(self)
