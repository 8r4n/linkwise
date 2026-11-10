# Julian / KiwiSDR candidate GIS data

This is an exploratory receiver shortlist for LW-MODEM-KIWI-001, not a reservation
or a propagation forecast. Julian town coordinates are an approximate transmit
reference, not the antenna location supplied by the operator.

## Sources and cross-reference

- Official directory: <http://kiwisdr.com/public/>. Direct requests failed during
  this selection; no claim of independently re-fetching that page is made.
- Retrieved public mirror: <http://rx.linkfanel.net/kiwisdr_com.js>. Its header says
  it is automatically generated from the official directory, with a source data
  timestamp of **2026-11-09 23:28:19 UTC**. The full local raw copy is under
  `docs/build/kiwi-candidate-map/`; its SHA-256 is in the candidate registry.
- Every selected endpoint was checked once through `/status`, without opening an
  audio stream. All returned active status; names and coordinates matched the
  selected directory entries. Checks and observation times are retained in JSON.
- Julian locality: [USGS GNIS-derived locality reference](https://www.topozone.com/california/san-diego-ca/city/julian/),
  33.0786552 N, 116.6019637 W. This is a planning point, not a surveyed RF station.
- Basemap: [Natural Earth](https://www.naturalearthdata.com/), global 110m land,
  coastline and boundaries; western-US inset 50m land, coastline and states.

Metadata availability is separate from recording permission, antenna behavior,
actual propagation, and future receiver capacity. GPS precision in a listing is
not an independently established positional accuracy. The directory can contain
aliases and multiple receivers at one site; those are not independent paths.

## Files

- `kiwisdr-candidates.json`: reviewed selection, source identity/hash, original
  directory values, rationales and metadata-only checks.
- `kiwisdr-candidates.csv`: flat candidate register with coordinates and endpoints.
- `kiwisdr-candidates.geojson`: WGS84 points, including the approximate transmitter.
- `kiwisdr-candidates.gpkg`: QGIS-compatible point and dense geodesic route layers.
- `kiwisdr-map-provenance.json`: projection, software versions and artifact hashes.

The map is generated with GeoPandas, Cartopy, PROJ/pyproj and Matplotlib. Open the
GeoPackage in QGIS and use the azimuthal-equidistant project CRS in the provenance
JSON to reproduce the Julian-centered view. Coordinate inputs are geographic
WGS84 (EPSG:4326); distances and initial bearings use the WGS84 ellipsoid. A route
crossing the antimeridian can look discontinuous in a longitude/latitude display;
use the stated Julian-centered projection. No HF hop geometry is modeled.

## Rebuild

Install the GIS dependencies in a dedicated Python environment, then run from
the repository root:

```sh
python3 -m pip install geopandas cartopy matplotlib pyproj
python3 tools/build-kiwisdr-candidate-map.py
python3 tools/build-kiwisdr-test-plan.py --build
```

The GIS renderer uses only the reviewed local registry; it does not connect to
receivers. Cartopy downloads Natural Earth layers on first use. Refreshing the
candidate registry is a separate reviewed data-acquisition step: preserve source
URL, source data timestamp, retrieval time, raw hash and original selected fields,
and repeat bounded metadata checks. Do not silently relabel a stale shortlist as
currently available. Changing the antenna location requires recomputing all
geodesics, the map and GIS layers.

## Repeatable accessibility check

Run `python3 tools/check-kiwisdr-candidates.py` from the repository root. It checks
the web interface and `/status` for each reviewed candidate using bounded curl
requests and saves timestamps, effective URLs, HTTP status, interface detection
and status fields to `kiwisdr-access-check.json`. Exit 0 requires every candidate
to serve a Kiwi HTML interface and report active/non-offline metadata. It does
not connect to receiver audio, establish owner consent or reserve capacity.
The map PDF links its IDs and register names to the candidate URLs.
