const slug = (name: string) => name.toLowerCase().replaceAll(" ", "-");

export const departments = [
  { name: "Clothing", sections: ["Jackets", "Rain shells", "Fleece", "Base layers", "Pants", "Shorts", "Socks", "Hats and gloves"] },
  { name: "Shoes", sections: ["Trail runners", "Hiking boots", "Approach shoes", "Sandals", "Camp shoes", "Insoles", "Laces"] },
  { name: "Packs", sections: ["Daypacks", "Backpacking packs", "Hydration packs", "Hip packs", "Dry bags", "Pack covers"] },
  { name: "Sleep", sections: ["Sleeping bags", "Quilts", "Sleeping pads", "Pillows", "Liners", "Hammocks"] },
  { name: "Shelter", sections: ["Backpacking tents", "Camping tents", "Tarps", "Bivy sacks", "Footprints", "Stakes and guylines"] },
  { name: "Kitchen", sections: ["Stoves", "Fuel", "Cookware", "Utensils", "Mugs and bottles", "Food storage", "Coffee"] },
  { name: "Navigation", sections: ["Maps", "Compasses", "GPS devices", "Watches", "Guidebooks"] },
  { name: "Safety", sections: ["First aid", "Satellite messengers", "Headlamps", "Bear spray", "Sun protection", "Insect repellent"] },
].map(({ name, sections }) => ({
  name,
  href: `/c/${slug(name)}`,
  sections: sections.map((section) => ({ name: section, href: `/c/${slug(name)}/${slug(section)}` })),
}));

export const help = [
  { title: "Help", links: ["Shipping", "Returns", "Sizing", "Order status", "Repairs", "Contact us"] },
  { title: "Membership", links: ["Join", "Dividend", "Member events", "Used gear", "Rentals", "Gift cards"] },
  { title: "About", links: ["Our story", "Stewardship", "Careers", "Stores", "Press", "Accessibility"] },
  { title: "Learn", links: ["Trip planning", "Gear guides", "Classes", "Trail reports", "Leave no trace", "Podcasts"] },
].map(({ title, links }) => ({
  title,
  links: links.map((link) => ({ name: link, href: `/${slug(title)}/${slug(link)}` })),
}));
