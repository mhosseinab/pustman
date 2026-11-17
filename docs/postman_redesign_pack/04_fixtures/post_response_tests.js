// Synthetic API design example, not run against a live API.
pm.test("Status is 200", () => pm.response.to.have.status(200));
const payload = pm.response.json();
pm.test("data is an array", () => pm.expect(payload.data).to.be.an("array"));
pm.test("page limit matches request", () => pm.expect(payload.page.limit).to.eql(Number(pm.request.url.query.get("limit"))));
pm.test("Every order has an id", () => {
  pm.expect(payload.data).to.be.an("array");
  payload.data.forEach(order => pm.expect(order.id).to.be.a("string"));
});
