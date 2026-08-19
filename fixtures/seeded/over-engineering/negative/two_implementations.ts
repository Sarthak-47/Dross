// Correct: the abstraction earns its place with two real implementors.
interface PaymentGateway {
  charge(amount: number): void;
}
class StripeGateway implements PaymentGateway {
  charge(amount: number): void { stripe.charge(amount); }
}
class PaypalGateway implements PaymentGateway {
  charge(amount: number): void { paypal.charge(amount); }
}
