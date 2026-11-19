const adjectives = 'Amber Apricot Breezy Bright Brisk Calm Cherry Clover Coral Cozy Dandy Dusky Ember Fuzzy Golden Hazy Honey Indigo Jolly Lilac Lunar Merry Misty Mossy Peachy Plum Quiet Rosy Silver Sleepy Soft Sunny Velvet Violet'.split(' ');
const nouns = 'Aster Badger Bluebird Bumblebee Chaffinch Clover Comet Cricket Daffodil Dahlia Dandelion Finch Firefly Fox Gecko Heron Kestrel Lantern Linnet Lynx Maple Marigold Meteor Minnow Moth Otter Pebble Petal Puffin Robin Sparrow Sprout Starling Thistle Willow Wren'.split(' ');
export function suggestName(previous: string) {
  const [a,n] = previous.split(' ');
  const old = adjectives.includes(a) && nouns.includes(n) ? adjectives.indexOf(a)*nouns.length+nouns.indexOf(n) : -1;
  let i = Math.floor(Math.random()*(adjectives.length*nouns.length-(old>=0?1:0)));
  if(old>=0 && i>=old) i++;
  return adjectives[Math.floor(i/nouns.length)]+' '+nouns[i%nouns.length];
}
