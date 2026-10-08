"""calibre_extensions.cPalmdoc: PalmDOC (LZ77) compression in Python."""


def decompress(data):
    out = bytearray()
    i, n = 0, len(data)
    while i < n:
        c = data[i]
        i += 1
        if c == 0:
            out.append(0)
        elif c <= 8:
            out.extend(data[i : i + c])
            i += c
        elif c <= 0x7F:
            out.append(c)
        elif c <= 0xBF:
            if i >= n:
                break
            c = (c << 8) | data[i]
            i += 1
            dist = (c >> 3) & 0x7FF
            length = (c & 7) + 3
            for _ in range(length):
                out.append(out[-dist])
        else:
            out.append(32)
            out.append(c ^ 0x80)
    return bytes(out)


def compress(data):
    out = bytearray()
    i, n = 0, len(data)
    while i < n:
        if i > 10 and n - i > 10:
            found = False
            for length in range(10, 2, -1):
                chunk = data[i : i + length]
                j = data.rfind(chunk, max(0, i - 2047), i)
                if j != -1:
                    dist = i - j
                    code = 0x8000 | (dist << 3) | (length - 3)
                    out.append(code >> 8)
                    out.append(code & 0xFF)
                    i += length
                    found = True
                    break
            if found:
                continue
        c = data[i]
        i += 1
        if c == 32 and i < n and 0x40 <= data[i] <= 0x7F:
            out.append(data[i] ^ 0x80)
            i += 1
        elif c == 0 or (c > 8 and c < 0x80):
            out.append(c)
        else:
            run = bytearray([c])
            while i < n and len(run) < 8 and (data[i] < 9 or data[i] >= 0x80):
                run.append(data[i])
                i += 1
            out.append(len(run))
            out.extend(run)
    return bytes(out)
