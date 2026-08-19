// Defect: an interface with exactly one implementor and no test double.
interface PaymentGateway {
  charge(amount: number): void;
}
class StripeGateway implements PaymentGateway {
  charge(amount: number): void { stripe.charge(amount); }
}
