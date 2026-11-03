export interface Review {
  author: string;
  rating: number;
  date: string;
  title: string;
  body: string;
}

export const reviews: Review[] = [
  {
    author: "Maya K.",
    rating: 5,
    date: "March 2, 2026",
    title: "Works in murky water",
    body: "I used it for a week in the Cascades, mostly from slow streams full of silt. The flow slowed by the fifth day, but one backflush with the syringe brought it right back. No stomach trouble at all, and the water tasted cleaner than from the tap at home.",
  },
  {
    author: "Tom R.",
    rating: 4,
    date: "February 18, 2026",
    title: "Light and simple",
    body: "It weighs next to nothing and threads onto the soft bottle that came with it. I knocked off a star because the bag is hard to fill from shallow puddles, so I carry a small scoop now. Otherwise it does exactly what it says.",
  },
  {
    author: "Priya S.",
    rating: 5,
    date: "January 29, 2026",
    title: "Third one I have owned",
    body: "My first one lasted four seasons before I dropped it on granite and cracked the housing. The new version has a tougher shell and a cap that stays attached. I keep one in the car and one in my pack, and I have given two away as gifts.",
  },
  {
    author: "Jonas W.",
    rating: 3,
    date: "January 4, 2026",
    title: "Do not let it freeze",
    body: "It worked well until a cold night on the ridge. I left it in the side pocket and the fibers froze, and after that I could not trust it. That is in the manual, so it is on me, but the warning could be printed on the filter itself.",
  },
  {
    author: "Aiko T.",
    rating: 5,
    date: "December 12, 2025",
    title: "Fast enough for a group",
    body: "Four of us shared it on a three day loop and nobody waited long. We hung the bag from a branch and let gravity do the work while we set up camp. Cleanup took a minute with the syringe.",
  },
  {
    author: "Luis M.",
    rating: 4,
    date: "November 30, 2025",
    title: "Good value",
    body: "It costs a fraction of a pump filter and weighs a tenth as much. The flow rate is lower than the box claims when the water is cold, closer to a liter every two minutes, but that is fine for one person.",
  },
  {
    author: "Hannah B.",
    rating: 5,
    date: "November 8, 2025",
    title: "Fits my bottles",
    body: "It screws onto the standard soda bottles I buy at trailheads, so I do not need special gear. I drink straight through it on long climbs and refill whenever I pass a creek, which means I carry half the water I used to.",
  },
  {
    author: "Ravi P.",
    rating: 4,
    date: "October 21, 2025",
    title: "Solid, but buy the spare gasket",
    body: "After a summer of daily use the gasket started to leak at the threads. A spare costs a few dollars and fixed it, and the company sent one free when I asked. I would buy it again.",
  },
];
