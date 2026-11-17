// Separate synthetic negative-auth profile.
pm.test("Rejects expired credentials with 401", () => pm.response.to.have.status(401));
