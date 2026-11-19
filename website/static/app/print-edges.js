// Each outline is drawn in fractions of the artwork's width and height.
// A few moving control points give the edge some life without shifting layout.
export function printEdge(shape, phase) {
  const swell = Math.sin(phase);
  const lean = Math.cos(phase);
  const n = value => value.toFixed(4);
  if (shape === 'pennant') {
    const top = n(.11 + .05 * swell);
    const bottom = n(.86 + .05 * swell);
    const notch = n(.77 + .04 * lean);
    return `M.018 .065 C.30 ${top} .54 -.015 .982 .052
      Q.945 .25 ${notch} ${n(.44 + .04 * swell)}
      Q.91 .65 .994 .868 C.71 ${bottom} .39 1.04 .051 .940
      C.059 .72 .002 .32 .018 .065Z`;
  }
  if (shape === 'tube') {
    return `M.155 .103 C.32 .036 .70 -.008 .851 .052
      C.989 .106 1.003 ${n(.28 + .04 * lean)} .976 .539
      C.951 .799 .900 .895 .754 .937
      C.568 .991 .274 .999 .128 .931
      C.005 .874 -.001 .685 .022 .439
      C.035 ${n(.254 + .04 * swell)} .050 .146 .155 .103Z`;
  }
  return `M.145 .154 C.256 .057 .420 .139 .531 .069
    C.724 -.047 .939 .031 .928 .232
    C.916 ${n(.379 + .05 * swell)} .998 .383 .994 .548
    C.985 .737 .822 .731 .760 .841
    C.658 1.010 .486 1.032 .379 .936
    C.289 ${n(.849 + .04 * lean)} .085 .938 .047 .776
    C.001 .593 .136 .551 .077 .407
    C.015 ${n(.272 - .04 * swell)} .051 .239 .145 .154Z`;
}
