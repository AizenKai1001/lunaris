"""Test vectors for crates/lunaris-net/src/scale/secure.rs, made with the `cryptography` package (OpenSSL's ChaCha20-Poly1305), XChaCha20 by HChaCha20 written here from draft-irtf-cfrg-xchacha.

    python3 tools/gen_secure_vectors.py        (from anywhere; needs `pip install cryptography`; writes crates/lunaris-net/src/scale/secure_vectors.txt)
"""
import os
import random
import struct

from cryptography.hazmat.primitives.ciphers.aead import ChaCha20Poly1305


def rotl(v, n):
    return ((v << n) & 0xFFFFFFFF) | (v >> (32 - n))


def hchacha20(key, nonce16):
    c = [0x61707865, 0x3320646E, 0x79622D32, 0x6B206574]
    s = c + list(struct.unpack("<8I", key)) + list(struct.unpack("<4I", nonce16))

    def qr(a, b, c_, d):
        s[a] = (s[a] + s[b]) & 0xFFFFFFFF; s[d] = rotl(s[d] ^ s[a], 16)
        s[c_] = (s[c_] + s[d]) & 0xFFFFFFFF; s[b] = rotl(s[b] ^ s[c_], 12)
        s[a] = (s[a] + s[b]) & 0xFFFFFFFF; s[d] = rotl(s[d] ^ s[a], 8)
        s[c_] = (s[c_] + s[d]) & 0xFFFFFFFF; s[b] = rotl(s[b] ^ s[c_], 7)

    for _ in range(10):
        qr(0, 4, 8, 12); qr(1, 5, 9, 13); qr(2, 6, 10, 14); qr(3, 7, 11, 15)
        qr(0, 5, 10, 15); qr(1, 6, 11, 12); qr(2, 7, 8, 13); qr(3, 4, 9, 14)
    return struct.pack("<8I", s[0], s[1], s[2], s[3], s[12], s[13], s[14], s[15])


rng = random.Random(8439)
lines = []
for n in [0, 1, 2, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 128, 129, 255, 256, 513, 1000, 1200]:
    key = bytes(rng.randrange(256) for _ in range(32))
    nonce = bytes(rng.randrange(256) for _ in range(12))
    aad = bytes(rng.randrange(256) for _ in range(rng.randrange(0, 40)))
    pt = bytes(rng.randrange(256) for _ in range(n))
    sealed = ChaCha20Poly1305(key).encrypt(nonce, pt, aad)
    lines.append(f"aead {key.hex()} {nonce.hex()} {aad.hex() or '-'} {pt.hex() or '-'} {sealed.hex()}")
for n in [0, 5, 64, 100, 300]:
    key = bytes(rng.randrange(256) for _ in range(32))
    nonce = bytes(rng.randrange(256) for _ in range(24))
    aad = bytes(rng.randrange(256) for _ in range(rng.randrange(0, 20)))
    pt = bytes(rng.randrange(256) for _ in range(n))
    sub = hchacha20(key, nonce[:16])
    sealed = ChaCha20Poly1305(sub).encrypt(b"\0\0\0\0" + nonce[16:], pt, aad)
    lines.append(f"xaead {key.hex()} {nonce.hex()} {aad.hex() or '-'} {pt.hex() or '-'} {sealed.hex()}")
# the RFC 8439 2.8.2 example, to be sure the generator itself is right
key = bytes(range(0x80, 0xA0))
nonce = bytes.fromhex("070000004041424344454647")
aad = bytes.fromhex("50515253c0c1c2c3c4c5c6c7")
pt = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it."
sealed = ChaCha20Poly1305(key).encrypt(nonce, pt, aad)
assert sealed[-16:].hex() == "1ae10b594f09e26a7e902ecbd0600691", sealed[-16:].hex()
lines.append(f"aead {key.hex()} {nonce.hex()} {aad.hex()} {pt.hex()} {sealed.hex()}")
# the XChaCha20-Poly1305 example of the draft (A.3.1)
xkey = bytes(range(0x80, 0xA0))
xnonce = bytes(range(0x40, 0x58))
xaad = bytes.fromhex("50515253c0c1c2c3c4c5c6c7")
sub = hchacha20(xkey, xnonce[:16])
xs = ChaCha20Poly1305(sub).encrypt(b"\0\0\0\0" + xnonce[16:], pt, xaad)
assert xs[-16:].hex() == "c0875924c1c7987947deafd8780acf49", xs[-16:].hex()
lines.append(f"xaead {xkey.hex()} {xnonce.hex()} {xaad.hex()} {pt.hex()} {xs.hex()}")
out = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "crates", "lunaris-net", "src", "scale", "secure_vectors.txt")
open(out, "w", newline="\n").write("\n".join(lines) + "\n")
print(len(lines), "vectors; the RFC 8439 and the XChaCha draft tags match")
