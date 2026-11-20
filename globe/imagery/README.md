# Orbital imagery

Both images are equirectangular WGS84 display textures. The simulation and the RF model do not consume their pixel values.

| Local asset | Source and processing |
| --- | --- |
| `terra-october.jpg` | NASA GIBS MODIS Terra corrected reflectance, October 20, 2026. Aqua corrected reflectance from the same date fills dark acquisition gaps, followed by Blue Marble shaded relief and bathymetry where both passes lack coverage. Full-world WMS requests used EPSG:4326, longitude −180 to +180, latitude −90 to +90, and 4,096 × 2,048 output pixels. Gap boundaries were feathered. |
| `night-context.jpg` | NASA Earth Observatory Black Marble 2016, downsampled from the global 3 km composite to 8,192 × 4,096. A dim blue rendering of the day mosaic supplies cloud and terrain context. City-light patterns originate in the VIIRS composite. Cloud illumination is illustrative. |

Source imagery: [NASA GIBS WMS](https://gibs.earthdata.nasa.gov/wms/epsg4326/best/wms.cgi?SERVICE=WMS&REQUEST=GetCapabilities) and [NASA Black Marble global image](https://assets.science.nasa.gov/content/dam/science/esd/eo/images/imagerecords/144000/144898/BlackMarble_2016_3km.jpg). GIBS layer identifiers are `MODIS_Terra_CorrectedReflectance_TrueColor`, `MODIS_Aqua_CorrectedReflectance_TrueColor`, and `BlueMarble_ShadedRelief_Bathymetry`.
