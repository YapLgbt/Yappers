// Shared by WebGL and Skia. Coordinates are in cloth units, independent of DPI.
uniform vec2 extent;
uniform float phase;
uniform float edition;

// sRGB stripe values. Sources: Wikimedia Commons flag SVGs for the rainbow,
// transgender, sunset lesbian, bisexual, nonbinary, pansexual, asexual,
// aromantic and genderfluid flags.
vec3 ink(float position, float palette) {
    float u = fract(position);
    if (palette < 0.5) {
        if (u < 1. / 6.) return vec3(228., 3., 3.) / 255.;
        if (u < 2. / 6.) return vec3(255., 140., 0.) / 255.;
        if (u < 3. / 6.) return vec3(255., 237., 0.) / 255.;
        if (u < 4. / 6.) return vec3(0., 128., 38.) / 255.;
        if (u < 5. / 6.) return vec3(0., 77., 255.) / 255.;
        return vec3(117., 7., 135.) / 255.;
    }
    if (palette < 1.5) {
        if (u < 1. / 5.) return vec3(91., 206., 250.) / 255.;
        if (u < 2. / 5.) return vec3(245., 169., 184.) / 255.;
        if (u < 3. / 5.) return vec3(255., 255., 255.) / 255.;
        if (u < 4. / 5.) return vec3(245., 169., 184.) / 255.;
        return vec3(91., 206., 250.) / 255.;
    }
    if (palette < 2.5) {
        if (u < 1. / 5.) return vec3(213., 45., 0.) / 255.;
        if (u < 2. / 5.) return vec3(255., 154., 86.) / 255.;
        if (u < 3. / 5.) return vec3(255., 255., 255.) / 255.;
        if (u < 4. / 5.) return vec3(211., 98., 164.) / 255.;
        return vec3(163., 2., 98.) / 255.;
    }
    if (palette < 3.5) {
        if (u < 2. / 5.) return vec3(214., 2., 112.) / 255.;
        if (u < 3. / 5.) return vec3(155., 79., 150.) / 255.;
        return vec3(0., 56., 168.) / 255.;
    }
    if (palette < 4.5) {
        if (u < 1. / 4.) return vec3(255., 244., 51.) / 255.;
        if (u < 2. / 4.) return vec3(255., 255., 255.) / 255.;
        if (u < 3. / 4.) return vec3(155., 89., 208.) / 255.;
        return vec3(45., 45., 45.) / 255.;
    }
    if (palette < 5.5) { // Pansexual
        if (u < 1. / 3.) return vec3(255., 33., 140.) / 255.;
        if (u < 2. / 3.) return vec3(255., 216., 0.) / 255.;
        return vec3(33., 177., 255.) / 255.;
    }
    if (palette < 6.5) { // Asexual
        if (u < .25) return vec3(0.);
        if (u < .5) return vec3(163. / 255.);
        if (u < .75) return vec3(1.);
        return vec3(128., 0., 128.) / 255.;
    }
    if (palette < 7.5) { // Aromantic
        if (u < .2) return vec3(61., 165., 66.) / 255.;
        if (u < .4) return vec3(167., 211., 121.) / 255.;
        if (u < .6) return vec3(1.);
        if (u < .8) return vec3(169. / 255.);
        return vec3(0.);
    }
    // Genderfluid
    if (u < .2) return vec3(255., 117., 162.) / 255.;
    if (u < .4) return vec3(245. / 255.);
    if (u < .6) return vec3(190., 24., 214.) / 255.;
    if (u < .8) return vec3(44. / 255.);
    return vec3(51., 62., 189.) / 255.;
}

vec3 weave(float course, float shuttle, float palette) {
    float y = fract(course);
    if (y < .59) return ink(1. - y / .59, palette);
    if (fract(shuttle) < .10) return ink(.96, palette);
    return ink((fract(shuttle) - .10) / .90, palette);
}

// A pair of oblong loops crosses four times. Swap their drawing order at
// alternating crossings and let the upper cord cast a shadow on the lower.
vec3 cord(vec3 under, float distanceToLoop, float palette, float order, float feather) {
    float width = .073;
    float d = abs(distanceToLoop);
    float stripe = clamp(.5 + order * distanceToLoop / (2. * width), .001, .999);
    vec3 yarn = ink(stripe, palette) * (.85 + .15 * sin(3.14159 * stripe));
    float shadow = 1. - smoothstep(width, width + .033, d);
    return mix(under * (1. - .35 * shadow), yarn, 1. - smoothstep(width-feather, width+feather, d));
}

vec3 loveKnot(vec2 p, float seed, float palette, float t, float scale) {
    float repeats = 1.75 + .28 * sin(seed);
    vec2 grid = p * repeats + vec2(.5);
    grid.x += .5 * mod(floor(grid.y), 2.);
    vec2 q = fract(grid) - .5;
    float angle = .49 + .16 * sin(seed + t);
    vec2 v = vec2(q.x*cos(angle) - q.y*sin(angle), q.x*sin(angle) + q.y*cos(angle));
    float across = length(vec2(max(abs(v.x)-.215, 0.), v.y)) - .135;
    float down = length(vec2(v.x, max(abs(v.y)-.215, 0.))) - .135;
    float over = step(0., v.x*v.y);
    float feather = min(.009, repeats / scale);
    vec3 ground = vec3(.135,.087,.164);
    vec3 lower = cord(ground, mix(across, down, over), palette, 1.-2.*over, feather);
    return cord(lower, mix(down, across, over), palette, 2.*over-1., feather);
}

// A complex log maps a flat strip of chambers between two spiral poles.
// One chamber row closes exactly after a turn, even across atan's branch cut.
vec3 nautilus(vec2 p, float seed, float palette, float t, float scale) {
    float angle = .25 * sin(seed);
    vec2 axis = vec2(cos(angle), sin(angle));
    vec2 a = p + axis * (.31 + .08*cos(seed)) + vec2(0., .06*sin(t));
    vec2 b = p - axis * (.33 + .07*sin(seed)) - vec2(0., .05*cos(t));
    float ra = max(length(a), .0001);
    float rb = max(length(b), .0001);
    float theta = (atan(a.y, a.x+.000001) - atan(b.y, b.x+.000001)) / 6.283185;
    float radial = log(ra/rb);
    float turn = radial * 1.28 - theta + .16*sin(t+seed);
    float shell = fract(turn);
    // Bow each septum towards the thick outer lip of the whorl.
    float aperture = theta*12. + radial*.9 + 1.32*shell*(1.-shell) + .28*shell;
    float chamber = fract(aperture);
    float panel = mod(floor(aperture), 12.);
    float tone = panel / 12. + .10*sin(6.283185*shell);
    vec3 enamel = ink(tone, palette);
    float pearl = pow(max(0., sin(3.14159*shell)*sin(3.14159*chamber)), .8);
    enamel = mix(enamel*.66, enamel*.88+vec3(.19,.16,.13), pearl);
    // Growth marks taper into the chamber face and disappear below pixel scale.
    float detail = smoothstep(5., 42., min(ra,rb)*scale);
    float marks = cos(6.283185*(chamber*17. + 1.4*sin(shell*3.14159)));
    enamel += .075 * smoothstep(.65,.98,marks) * (1.-chamber) * detail;
    float join = min(shell,1.-shell);
    float rib = min(chamber,1.-chamber);
    vec3 midnight = vec3(.105,.073,.14);
    enamel = mix(midnight, enamel, smoothstep(.009,.028,rib));
    enamel = mix(midnight, enamel, smoothstep(.012,.033,join));
    float lip = 1.-smoothstep(.01,.029,abs(shell-.055));
    enamel = mix(enamel, vec3(.98,.83,.62), .92*lip);
    return mix(midnight, enamel, smoothstep(1.,5.,min(ra,rb)*scale));
}

// Smooth lattice noise with an arithmetic hash so Skia and WebGL agree.
float grainHash(vec2 cell) {
    vec2 h = fract(cell*vec2(.1031,.11369));
    h += dot(h, h.yx+19.19);
    return fract(h.x*h.y);
}

float terrain(vec2 p) {
    vec2 id = floor(p);
    vec2 f = fract(p);
    vec2 w = f*f*(3.-2.*f);
    return mix(mix(grainHash(id),grainHash(id+vec2(1.,0.)),w.x),
               mix(grainHash(id+vec2(0.,1.)),grainHash(id+vec2(1.)),w.x), w.y);
}

vec3 estuary(vec2 p, float pageY, float seed, float palette, float t, float scale) {
    vec2 q = p*2.3 + vec2(1.7*seed, 3.1*seed);
    // Bend a whole watershed first; cut smaller inlets into its banks afterwards.
    vec2 tide = .10*vec2(cos(t),sin(t));
    float basin = terrain(q*.83+tide);
    vec2 bank = q + vec2(2.1*basin, .95*sin(2.8*q.x+seed)+basin);
    float h = .65*terrain(bank+tide) + .25*terrain(bank*2.1-tide)
            + .10*terrain(bank*4.3+3.);
    // Pin the first and last bands to the page edges. Terrain stretches the
    // intervening bands into coves, so a shallow crop still contains the flag.
    float v = clamp(pageY, 0., 1.);
    float stretch = exp(6.8 * (.46-h));
    float course = v / (v + (1.-v)*stretch);
    vec3 pigment = ink(min(course,.99999),palette);

    // Depth is a change in exposure of the same pigment. Narrow contour
    // marks describe the relief without replacing large areas of flag color.
    float coast = smoothstep(.421,.427,h);
    float depth = smoothstep(.19,.426,h);
    float soundings = pow(.5+.5*cos((h-.428)*530.),6.);
    vec3 water = pigment*(.78+.17*depth+.05*soundings);
    float ledge = fract((h-.2)*13.);
    float line = 1.-smoothstep(.018,.095,abs(fract(ledge*3.)-.5));
    vec3 land = pigment*(.86+.14*smoothstep(.02,.16,ledge));
    land = mix(land,vec3(1.),line*.22*smoothstep(140.,380.,scale));
    vec3 shore = mix(water,land,coast);
    return mix(shore,vec3(1.),.62*(1.-smoothstep(.001,.005,abs(h-.426))));
}

vec3 fabricWithPalette(vec2 pixel, float palette) {
    float scale = max(extent.y, extent.x * .57);
    vec2 p = (pixel - .5 * extent) / scale;
    float family = mod(edition, 6.);
    float seed = mod(edition * .618034, 6.28318);
    float t = phase;
    // Advect both thread directions through the same slow fold field.
    float fold = sin(6.2*p.x + 3.3*p.y + sin(seed) + t);
    p.y += .044 * fold + .012 * sin(15.*p.x - 3.*p.y - t);
    p.x += .032 * sin(7.*p.y + seed - t);
    vec3 dye;
    if (family < .5) {
        float row = p.y * (5.5 + .65*sin(seed)) + .35*sin(3.*p.x + seed);
        float shuttle = p.x * 17. + .31*sin(14.*p.x + floor(row)*2.1 + seed)
                      + .19 * fract(row) * sin(floor(row) + seed + t);
        dye = weave(row, shuttle, palette);
    } else if (family < 1.5) {
        p -= .09 * vec2(sin(seed), cos(seed));
        float a = atan(p.y, p.x + .000001);
        float r = length(p);
        float lobes = 5. + mod(floor(edition / 3.), 3.);
        float ring = r * (8.2 + .85*cos(lobes*a + 3.*r + .7*sin(t) + seed));
        float petals = 6. * (floor(ring) + 2.);
        dye = weave(ring, a * petals / 6.283185 + .24*sin(ring + t), palette);
    } else if (family < 2.5) {
        vec2 grid = p * (4.6 + .6*cos(seed)) + seed;
        vec2 tile = floor(grid);
        vec2 q = fract(grid);
        float flip = fract(sin(dot(tile, vec2(12.9898,78.233)) + seed)*4375.54);
        if (flip > .5) q.x = 1. - q.x;
        float farCorner = step(1., q.x + q.y);
        float r = length(q - farCorner);
        float stripe = (r - .19) / .62;
        float reverse = mod(tile.x + tile.y + step(.5,flip), 2.);
        dye = stripe < 0. || stripe > 1. ? vec3(.19,.14,.23)
              : ink(mix(stripe, 1.-stripe, reverse), palette);
        // Short running stitches follow the bend just inside each hem.
        vec2 radial = q - farCorner;
        float hem = min(abs(stripe - .07), abs(stripe - .93));
        float dash = cos(48. * atan(radial.y, radial.x + .000001));
        dye = mix(dye, vec3(1.,.92,.80), .64*(1.-smoothstep(.008,.023,hem))*smoothstep(.1,.65,dash));
    } else if (family < 3.5) {
        dye = loveKnot(p, seed, palette, t, scale);
    } else if (family < 4.5) {
        dye = nautilus(p, seed, palette, t, scale);
    } else {
        return estuary(p, pixel.y / extent.y, seed, palette, t, scale);
    }
    // Broad satin highlights cross the stripes, independent of ink color.
    return dye * (.94 + .06*fold) + .035 * pow(.5 + .5*fold, 5.);
}

vec3 fabric(vec2 pixel) {
    return fabricWithPalette(pixel, mod(edition, 5.));
}

// Finish the flattened color image. Every family uses the same virtual stock.
uniform float finishMode;

float screenInk(vec2 sheet, vec2 axis, float density) {
    vec2 grid = vec2(dot(sheet,axis), dot(sheet,vec2(-axis.y,axis.x)));
    float threshold = .5 + .25 * (cos(6.283185*grid.x) + cos(6.283185*grid.y));
    // Soften the screened boundary to simulate ink soaking into the stock.
    return mix(density, smoothstep(threshold-.13, threshold+.13, density), .49);
}

vec3 finishedPrint(vec2 pixel) {
    float unit = max(1., min(extent.x, extent.y*2.) / 520.);
    vec2 sheet = pixel / unit;
    float tooth = grainHash(floor(sheet*1.23)) - .5;
    vec3 base = vec3(0.);
    vec3 scatter = vec3(0.);
    vec3 plates = vec3(0.);
    // Re-sample the same scene at the print head or lens offsets. Keep the
    // procedural scene call in one loop so both backends share its program.
    for (int tap=0; tap<4; tap++) {
        vec2 delta = vec2(0.);
        if (finishMode < .5) {
            if (tap >= 2) break;
            delta = vec2(tap == 0 ? -.24 : .24);
        } else if (finishMode < 1.5) {
            if (tap >= 3) break;
            if (tap == 1) delta = vec2(1.65,-.72) * unit;
            if (tap == 2) delta = vec2(-1.05,.88) * unit;
        } else if (tap > 0) {
            float a = 2.094395 * float(tap-1) + .42;
            delta = vec2(cos(a),sin(a)) * 3.1 * unit;
        }
        vec3 sampleColor = clamp(fabric(pixel + delta), 0., 1.);
        if (tap == 0) base = sampleColor;
        else scatter += sampleColor;
        if (tap == 0) plates.r = 1.-sampleColor.r;
        if (tap == 1) plates.g = 1.-sampleColor.g;
        if (tap == 2) plates.b = 1.-sampleColor.b;
    }
    if (finishMode < .5) return (base + scatter)*.5;
    if (finishMode < 1.5) {
        vec2 screen = sheet / 4.1;
        float cyan = screenInk(screen, vec2(.966,.259), plates.r);
        float pink = screenInk(screen, vec2(.707,-.707), plates.g);
        float gold = screenInk(screen, vec2(.259,.966), plates.b);
        vec3 paper = vec3(1.,.957,.864) * (1. + .065*tooth);
        vec3 impression = paper * mix(vec3(1.), vec3(.14,.75,.81), cyan)
                                * mix(vec3(1.), vec3(.95,.20,.55), pink)
                                * mix(vec3(1.), vec3(1.,.83,.11), gold);
        return impression;
    }
    // Diffusion scatters bright exposures into neighboring shadows. The
    // red-sensitive emulsion leaves a warmer fringe along those boundaries.
    vec3 soft = scatter / 3.;
    vec3 luma = vec3(.2126,.7152,.0722);
    float glow = max(0., dot(soft-base,luma));
    vec3 exposed = mix(base,soft,.17) + glow*vec3(.63,.17,.05);
    float light = dot(exposed,luma);
    vec3 negative = mix(vec3(.045,.064,.080), vec3(1.,.94,.84),
                        smoothstep(vec3(-.08),vec3(1.04),exposed));
    float frame = floor(phase * 32.);
    float grain = grainHash(floor(sheet*1.35)+frame*vec2(13.,7.)) - .5;
    vec2 gate = (pixel / extent - .5)*2.;
    return negative * (1.-.075*dot(gate,gate))
           + grain*(.035+.095*(1.-abs(2.*light-1.)));
}
