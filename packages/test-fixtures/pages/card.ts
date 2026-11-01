// Draws the bar chart into the card's <canvas>: content only Chrome's pixels can provide.
const canvas = document.querySelector<HTMLCanvasElement>("[data-testid=chart]");
const ctx = canvas?.getContext("2d");
if (ctx) {
  [30, 70, 45, 85, 60].forEach((v, i) => {
    ctx.fillStyle = i % 2 ? "#10b981" : "#0ea5e9";
    ctx.fillRect(8 + i * 30, 90 - v, 22, v);
  });
}
