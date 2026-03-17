# TODO: oxigdal-sensors

## High Priority
- [ ] Add Sentinel-1 SAR processing (sigma0, gamma0 calibration)
- [ ] Implement 6S radiative transfer model for atmospheric correction
- [ ] Add MODIS cloud masking (MOD35 algorithm)
- [ ] Implement surface reflectance computation (LEDAPS/LaSRC equivalent)
- [ ] Add Sentinel-2 Scene Classification Layer (SCL) integration
- [ ] Implement thermal band processing (brightness temperature, LST)

## Medium Priority
- [ ] Add WorldView-2/3 sensor definitions and spectral response functions
- [ ] Implement SAR speckle filtering (Lee, Frost, Gamma-MAP)
- [ ] Add VIIRS sensor support (DNB, I-bands, M-bands)
- [ ] Implement hyperspectral index computation (PRI, RENDVI, REP)
- [ ] Add Sentinel-2 super-resolution (20m to 10m bands)
- [ ] Implement radiometric cross-calibration between sensors
- [ ] Add support for PlanetScope and SkySat sensor definitions
- [ ] Implement BRDF correction (Ross-Li kernel model)
- [ ] Add topographic correction (Minnaert, C-correction, SCS+C)

## Low Priority / Future
- [ ] Add radar polarimetric decomposition (Freeman-Durden, Cloude-Pottier)
- [ ] Implement InSAR coherence computation
- [ ] Add multi-temporal SAR change detection
- [ ] Implement spectral unmixing (endmember extraction, abundance estimation)
- [ ] Add UAV/drone sensor calibration support
- [ ] Implement SIF (Solar-Induced Fluorescence) retrieval
