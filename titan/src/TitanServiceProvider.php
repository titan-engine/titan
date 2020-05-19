<?php

namespace Titan;

use Illuminate\Support\Facades\Log;
use Illuminate\Support\ServiceProvider;
use Titan\Commands\MakeExtension;
use Titan\Commands\RefreshExtensionsCache;
use Titan\Commands\InstallTitan;
use Titan\Commands\PublishTitanResources;
use Titan\Commands\SuperAdmin;
use Titan\Commands\UpdateTitan;
use Titan\Commands\VerifyTitan;
use Titan\Models\Settings;
use Titan\Observers\StatObserver;
use Titan\Providers\BanUserServiceProvider;
use Titan\Providers\ExtensionServiceProvider;

class TitanServiceProvider extends ServiceProvider
{

    /**
     * Bootstrap services.
     *
     * @return void
     */
    public function boot()
    {

        $this->loadRoutesFrom(__DIR__ . '/routes.php');

        $this->loadMigrationsFrom( __DIR__ . '/../database/migrations');

        $this->loadViewsFrom(__DIR__ . '/../resources/views', 'titan');

        $this->commands([
            InstallTitan::class,
            PublishTitanResources::class,
            UpdateTitan::class,
            RefreshExtensionsCache::class,
            SuperAdmin::class,
            MakeExtension::class,
        ]);

        $this->publishes([
//            __DIR__ . '/../resources/views' =>  resource_path('views/vendor/titan'),
            __DIR__ . '/../resources/sass' => resource_path('sass'),
            __DIR__ . '/../resources/js' => resource_path('js'),
        ], 'titan');

        \Gate::before(function ($user, $ability) {
            return $user->hasRole('Super Admin') ? true : null;
        });

        \View::composer(['titan::game.*', 'titan::layouts.game'], function ($view) {
            $character = \Auth::user()->character;
            $view->with('character', $character);
        });

        Stat::observe(StatObserver::class);

        $this->app->register(ExtensionServiceProvider::class);
    }

    /**
     * Register services.
     *
     * @return void
     */
    public function register()
    {
        $this->app->singleton('menu', function () {
            $menu = Menu::with('items')->whereEnabled(true)->get();
            return $menu;
        });

        $this->app->singleton('settings', function () {
            return Settings::all();
        });

        $this->mergeConfigFrom(
            __DIR__ . '/../config/titan.php', 'titan'
        );

        $this->mergeConfigFrom(
            __DIR__ . '/../config/game.php', 'game'
        );

        $this->app->register(BanUserServiceProvider::class);

        /**
         * Weird hack for loading factories from titan/database/factories
         * Apparently the autoload factories in composer doens't work as I expected?
         */
        $this->app->singleton(\Illuminate\Database\Eloquent\Factory::class, function ($app) {
            $faker = $app->make(\Faker\Generator::class);
            return \Illuminate\Database\Eloquent\Factory::construct($faker, base_path('titan/database/factories'));
        });

    }
}
