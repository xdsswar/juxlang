public class Main {
    static String greet(String name) { return greet(name, "Hello", 1, false); }
    static String greet(String name, String greeting) { return greet(name, greeting, 1, false); }

    static String greet(String name, String greeting, int times, boolean shout) {
        String line = "";
        for (int i = 0; i < times; i++) {
            if (i > 0) { line += " "; }
            line += greeting + ", " + name;
        }
        return shout ? line.toUpperCase() + "!" : line;
    }

    static int sum(int... values) {
        int total = 0;
        for (int v : values) {
            total += v;
        }
        return total;
    }

    static String joinAll(String sep, String... parts) {
        String out = "";
        for (int i = 0; i < parts.length; i++) {
            if (i > 0) { out += sep; }
            out += parts[i];
        }
        return out;
    }

    static class Address {
        String city;
        String zip;
        Address(String city, String zip) { this.city = city; this.zip = zip; }
    }

    static class User {
        String name;
        Address address;
        User manager;
        Integer age;
        User(String name, Address address, User manager, Integer age) {
            this.name = name;
            this.address = address;
            this.manager = manager;
            this.age = age;
        }
    }

    static String city(User u) {
        if (u == null || u.address == null) { return "unknown"; }
        return u.address.city;
    }

    static String zip(User u) {
        Address a = u.address;
        if (a == null) { return "no address"; }
        return a.zip != null ? a.zip : "no zip";
    }

    static int managerChainLength(User u) {
        int n = 0;
        User cur = u.manager;
        while (cur != null) {
            n++;
            cur = cur.manager;
        }
        return n;
    }

    static int ageOrThrow(User u) {
        if (u.age == null) {
            throw new IllegalStateException(u.name + " has no age");
        }
        return u.age;
    }

    public static void main(String[] args) {
        System.out.println(greet("Ada"));
        System.out.println(greet("Ada", "Hi"));
        System.out.println(greet("Ada", "Hello", 2, false));
        System.out.println(greet("Bob", "Hey", 1, true));
        System.out.println(greet("Cy", "Yo", 3, false));

        System.out.println(sum());
        System.out.println(sum(4));
        System.out.println(sum(1, 2, 3, 4));
        int[] xs = {10, 20, 30};
        System.out.println(sum(xs));
        System.out.println(joinAll("-", "a", "b", "c"));
        System.out.println(joinAll(", "));

        User ceo = new User("Grace", new Address("Arlington", "22201"), null, 85);
        User vp = new User("Linus", new Address("Portland", null), ceo, null);
        User dev = new User("Ada", null, vp, 36);
        User nobody = null;

        System.out.println(city(dev) + " / " + city(vp) + " / " + city(nobody));
        System.out.println(zip(ceo) + " / " + zip(vp) + " / " + zip(dev));
        System.out.println("chain " + managerChainLength(dev) + " " + managerChainLength(ceo));
        System.out.println(dev.manager.manager.name);
        System.out.println((dev.age != null ? dev.age : 0) + (vp.age != null ? vp.age : 0));
        try {
            System.out.println(ageOrThrow(dev));
            System.out.println(ageOrThrow(vp));
        } catch (IllegalStateException e) {
            System.out.println("error: " + e.getMessage());
        }
    }
}
