const compactLayout = matchMedia('(max-width: 800px)');
const contents = document.querySelector('.section-index details');
function setIndexLayout() { contents.open = !compactLayout.matches; }
setIndexLayout();
compactLayout.addEventListener('change', setIndexLayout);
