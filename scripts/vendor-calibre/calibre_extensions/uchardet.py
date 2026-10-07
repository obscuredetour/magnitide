"""calibre_extensions.uchardet: encoding detection, through charset-normalizer."""


def detect(data):
    try:
        from charset_normalizer import from_bytes

        best = from_bytes(bytes(data)).best()
        return best.encoding if best else ''
    except Exception:
        return ''
