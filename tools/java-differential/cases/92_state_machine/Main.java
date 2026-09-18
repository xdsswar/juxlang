import java.util.ArrayList;

public class Main {
    enum Light { Red, Green, Yellow }

    static Light next(Light l) {
        return switch (l) {
            case Red -> Light.Green;
            case Green -> Light.Yellow;
            case Yellow -> Light.Red;
        };
    }

    static int seconds(Light l) {
        switch (l) {
            case Red -> { return 30; }
            case Green -> { return 25; }
            default -> { return 5; }
        }
    }

    enum Order { Created, Paid, Shipped, Delivered, Cancelled }

    static Order apply(Order o, String event) {
        if (event.equals("cancel")) {
            return (o == Order.Shipped || o == Order.Delivered) ? o : Order.Cancelled;
        }
        return switch (o) {
            case Created -> event.equals("pay") ? Order.Paid : o;
            case Paid -> event.equals("ship") ? Order.Shipped : o;
            case Shipped -> event.equals("deliver") ? Order.Delivered : o;
            default -> o;
        };
    }

    public static void main(String[] args) {
        Light l = Light.Red;
        int elapsed = 0;
        for (int i = 0; i < 7; i++) {
            elapsed = elapsed + seconds(l);
            System.out.println(l + " for " + seconds(l));
            l = next(l);
        }
        System.out.println("elapsed " + elapsed);

        String[] events = {"pay", "deliver", "ship", "cancel", "deliver", "pay"};
        Order o = Order.Created;
        ArrayList<String> history = new ArrayList<>();
        for (String e : events) {
            Order before = o;
            o = apply(o, e);
            history.add(before + " --" + e + "--> " + o);
        }
        for (String h : history) {
            System.out.println(h);
        }

        Order other = apply(apply(Order.Created, "pay"), "cancel");
        System.out.println(other);
        System.out.println(other == Order.Cancelled);
    }
}
