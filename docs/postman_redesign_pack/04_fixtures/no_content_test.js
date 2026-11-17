// Separate synthetic delete/no-content profile.
pm.test("Status is 204", () => pm.response.to.have.status(204));
