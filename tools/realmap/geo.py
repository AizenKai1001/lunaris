"""WGS84 <-> UTM (transverse Mercator, Krueger series) without any dependency: the real-world map tools work in UTM metres, where one unit is one metre on the ground, and
the engine's x is east, z is south (-north), y is up.
"""
import math

A = 6378137.0
F = 1 / 298.257223563
K0 = 0.9996
N = F / (2 - F)
# Krueger series coefficients
A_HAT = A / (1 + N) * (1 + N * N / 4 + N**4 / 64)
ALPHA = [
    N / 2 - 2 * N**2 / 3 + 5 * N**3 / 16,
    13 * N**2 / 48 - 3 * N**3 / 5,
    61 * N**3 / 240,
]
BETA = [
    N / 2 - 2 * N**2 / 3 + 37 * N**3 / 96,
    N**2 / 48 + N**3 / 15,
    17 * N**3 / 480,
]
E = math.sqrt(F * (2 - F))


def zone_of(lon):
    return int((lon + 180) // 6) + 1


def epsg_of(lat, lon):
    return (32600 if lat >= 0 else 32700) + zone_of(lon)


def forward(lat, lon, zone=None):
    zone = zone or zone_of(lon)
    lon0 = math.radians((zone - 1) * 6 - 180 + 3)
    phi, lam = math.radians(lat), math.radians(lon) - lon0
    t = math.sinh(math.atanh(math.sin(phi)) - E * math.atanh(E * math.sin(phi)))
    xi = math.atan2(t, math.cos(lam))
    eta = math.atanh(math.sin(lam) / math.sqrt(1 + t * t))
    x = eta + sum(ALPHA[j] * math.cos(2 * (j + 1) * xi) * math.sinh(2 * (j + 1) * eta) for j in range(3))
    y = xi + sum(ALPHA[j] * math.sin(2 * (j + 1) * xi) * math.cosh(2 * (j + 1) * eta) for j in range(3))
    easting = 500000.0 + K0 * A_HAT * x
    northing = K0 * A_HAT * y + (0 if lat >= 0 else 10000000.0)
    return easting, northing


def inverse(easting, northing, zone, north=True):
    lon0 = math.radians((zone - 1) * 6 - 180 + 3)
    x = (easting - 500000.0) / (K0 * A_HAT)
    y = (northing - (0 if north else 10000000.0)) / (K0 * A_HAT)
    xi_p = y - sum(BETA[j] * math.sin(2 * (j + 1) * y) * math.cosh(2 * (j + 1) * x) for j in range(3))
    eta_p = x - sum(BETA[j] * math.cos(2 * (j + 1) * y) * math.sinh(2 * (j + 1) * x) for j in range(3))
    chi = math.asin(math.sin(xi_p) / math.cosh(eta_p))
    # conformal latitude to geodetic latitude
    phi = chi
    for _ in range(6):
        phi = math.asin(math.tanh(math.atanh(math.sin(chi)) + E * math.atanh(E * math.sin(phi))))
    lam = lon0 + math.atan2(math.sinh(eta_p), math.cos(xi_p))
    return math.degrees(phi), math.degrees(lam)


if __name__ == "__main__":
    # Stowe, Vermont: UTM 18N, a known point to check the series against
    e, n = forward(44.4654, -72.6874)
    lat, lon = inverse(e, n, 18)
    print(f"{e:.2f} {n:.2f}  back: {lat:.6f} {lon:.6f}")
    assert abs(lat - 44.4654) < 1e-7 and abs(lon + 72.6874) < 1e-7
    # the central meridian at the equator is the false easting
    e0, n0 = forward(0.0, -75.0, 18)
    assert abs(e0 - 500000.0) < 1e-6 and abs(n0) < 1e-6
    print("ok")
