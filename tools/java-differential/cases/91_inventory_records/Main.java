import java.util.ArrayList;
import java.util.HashMap;

public class Main {
    record Item(String sku, String name, int qty, double price) {}

    static double total(ArrayList<Item> items) {
        double sum = 0.0;
        for (Item it : items) {
            sum = sum + it.qty() * it.price();
        }
        return sum;
    }

    static ArrayList<Item> lowStock(ArrayList<Item> items, int limit) {
        ArrayList<Item> out = new ArrayList<>();
        for (Item it : items) {
            if (it.qty() < limit) { out.add(it); }
        }
        return out;
    }

    static Item restock(Item it, int more) {
        return new Item(it.sku(), it.name(), it.qty() + more, it.price());
    }

    public static void main(String[] args) {
        ArrayList<Item> items = new ArrayList<>();
        items.add(new Item("A1", "bolt", 120, 0.25));
        items.add(new Item("B2", "nut", 8, 0.1));
        items.add(new Item("C3", "gear", 3, 12.5));
        items.add(new Item("D4", "belt", 15, 7.75));

        System.out.println(total(items));
        ArrayList<Item> low = lowStock(items, 10);
        System.out.println(low.size());
        for (Item it : low) {
            System.out.println(it.name() + " x" + it.qty());
        }

        Item gear = items.get(2);
        Item more = restock(gear, 7);
        System.out.println(gear.qty() + " -> " + more.qty());
        System.out.println(gear.equals(new Item("C3", "gear", 3, 12.5)));
        System.out.println(gear.equals(more));
        items.set(2, more);
        System.out.println(total(items));

        HashMap<String, Integer> bands = new HashMap<>();
        for (Item it : items) {
            String band = it.price() < 1.0 ? "cheap" : (it.price() < 10.0 ? "mid" : "dear");
            if (bands.containsKey(band)) {
                bands.put(band, bands.get(band) + 1);
            } else {
                bands.put(band, 1);
            }
        }
        System.out.println("cheap=" + bands.get("cheap") + " mid=" + bands.get("mid") + " dear=" + bands.get("dear"));
    }
}
