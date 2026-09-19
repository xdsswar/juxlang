import java.util.ArrayList;
import java.util.HashMap;
import java.util.Map;
import java.util.TreeMap;

public class Main {
    sealed interface Item permits Book, Gadget, Food {}
    record Book(String title, long cents, int pages) implements Item {}
    record Gadget(String name, long cents, int warrantyMonths) implements Item {}
    record Food(String name, long cents, int daysLeft) implements Item {}

    record Sku(String kind, int number) {}

    static long price(Item item) {
        return switch (item) {
            case Book(String t, long c, int p) when p > 500 -> c - c / 10;
            case Book(String t, long c, int p) -> c;
            case Gadget(String n, long c, int w) when w >= 24 -> c + 1500;
            case Gadget(String n, long c, int w) -> c;
            case Food(String n, long c, int d) when d <= 1 -> c / 2;
            case Food(String n, long c, int d) -> c;
        };
    }

    static String label(Item item) {
        return switch (item) {
            case Book b -> "book " + b.title();
            case Gadget g -> "gadget " + g.name();
            case Food f -> "food " + f.name();
        };
    }

    static String money(long cents) {
        long rest = cents % 100;
        return (cents / 100) + "." + (rest < 10 ? "0" : "") + rest;
    }

    public static void main(String[] args) {
        TreeMap<String, Integer> stock = new TreeMap<>();
        ArrayList<Item> catalog = new ArrayList<>();
        catalog.add(new Book("Dune", 1899, 688));
        catalog.add(new Book("Poems", 1200, 90));
        catalog.add(new Gadget("Drill", 8900, 36));
        catalog.add(new Gadget("Lamp", 2500, 12));
        catalog.add(new Food("Bread", 350, 1));
        catalog.add(new Food("Rice", 900, 300));

        long total = 0;
        for (Item item : catalog) {
            long p = price(item);
            total += p;
            System.out.println(label(item) + ": " + money(p));
            String key = label(item);
            stock.put(key, stock.getOrDefault(key, 0) + 1);
        }
        System.out.println("total " + money(total));

        HashMap<Sku, String> bins = new HashMap<>();
        bins.put(new Sku("book", 1), "A1");
        bins.put(new Sku("food", 7), "C3");
        System.out.println("sku book-1 in " + bins.get(new Sku("book", 1)));
        System.out.println("same sku: " + new Sku("food", 7).equals(new Sku("food", 7)));

        Gadget lamp = new Gadget("Lamp", 2500, 12);
        Gadget better = new Gadget(lamp.name(), lamp.cents(), 24);
        System.out.println("lamp " + money(price(lamp)) + " -> " + money(price(better)));
        System.out.println("original warranty " + lamp.warrantyMonths());

        for (Map.Entry<String, Integer> entry : stock.entrySet()) {
            System.out.println(entry.getKey() + " x" + entry.getValue());
        }
    }
}
